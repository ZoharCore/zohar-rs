use bevy::prelude::*;
use zohar_domain::entity::MovementAnimation;

use super::action_pipeline::{
    Action, apply_action, build_player_attack_action, build_player_move_action,
};
use super::aggro::{MobAggroDispatch, MobAggroDispatchBuffer};
use super::query::validate_player_attack;
use super::state::{
    LocalTransform, MapSpatial, MobAggro, MobRef, NetEntityId, PlayerActivityComp, PlayerCommand,
    PlayerCommandQueue, PlayerMarker, PlayerMovementAnimation, RuntimeState,
};

pub(crate) fn process_player_actions(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let player_entities = super::players::player_entities_on_map(world);
    let mut dispatches = Vec::new();
    let mut spatial_dirty = false;

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        let Some((player_id, attacker_net_id)) = ({
            let entity = world.entity(player_entity);
            match (entity.get::<PlayerMarker>(), entity.get::<NetEntityId>()) {
                (Some(marker), Some(net_id)) => Some((marker.player_id, net_id.net_id)),
                _ => None,
            }
        }) else {
            continue;
        };

        let commands = {
            let mut entity = world.entity_mut(player_entity);
            let Some(mut queue) = entity.get_mut::<PlayerCommandQueue>() else {
                continue;
            };
            std::mem::take(&mut queue.0)
        };

        if !crate::runtime::actor_life::actor_can_act(world, player_entity) {
            continue;
        }

        for command in commands {
            match command {
                PlayerCommand::Move {
                    kind,
                    arg,
                    rot,
                    target,
                    ts,
                } => {
                    if let Some(action) = build_player_move_action(
                        world,
                        world.resource::<super::state::SharedConfig>(),
                        world.resource::<super::state::MapConfig>().local_size,
                        player_entity,
                        player_id,
                        kind,
                        arg,
                        rot,
                        target,
                        ts,
                    ) {
                        spatial_dirty |= matches!(action, Action::PlayerMotion { .. });
                        apply_action(world, action);
                    }
                }
                PlayerCommand::SetMovementAnimation(animation) => {
                    set_player_movement_animation(world, player_entity, animation);
                }
                PlayerCommand::SelectTarget { target } => {
                    super::target::select_target(
                        world,
                        map_entity,
                        player_entity,
                        attacker_net_id,
                        target,
                    );
                }
                PlayerCommand::Attack { target, attack } => {
                    let Some(attacker_pos) = world
                        .entity(player_entity)
                        .get::<LocalTransform>()
                        .map(|transform| transform.pos)
                    else {
                        continue;
                    };
                    let Some(target_entity) = validate_player_attack(
                        world,
                        map_entity,
                        attacker_net_id,
                        attacker_pos,
                        target,
                    ) else {
                        continue;
                    };

                    if let Some(action) = build_player_attack_action(
                        world,
                        player_entity,
                        target,
                        target_entity,
                        attack,
                    ) {
                        apply_action(world, action);
                    }

                    if !world.entity(target_entity).contains::<MobRef>() {
                        continue;
                    }

                    dispatches.push(MobAggroDispatch {
                        attacked_mob_entity: target_entity,
                        aggro: MobAggro::ProvokedBy {
                            attacker: attacker_net_id,
                        },
                    });
                }
            }
        }
    }

    if spatial_dirty && let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>()
    {
        spatial.0.maintain();
    }
    world
        .resource_mut::<MobAggroDispatchBuffer>()
        .0
        .extend(dispatches);
}

fn set_player_movement_animation(
    world: &mut World,
    player_entity: Entity,
    animation: MovementAnimation,
) {
    let now = world.resource::<RuntimeState>().sim_now;
    let mut player_entity_ref = world.entity_mut(player_entity);
    {
        let Some(mut current) = player_entity_ref.get_mut::<PlayerMovementAnimation>() else {
            return;
        };
        if current.0 != animation {
            current.0 = animation;
        }
    }

    if let Some(mut activity) = player_entity_ref.get_mut::<PlayerActivityComp>() {
        activity.preferred_movement_animation = animation;
        if animation == MovementAnimation::Walk {
            activity.last_walk_started_at = Some(now);
        }
    }
}
