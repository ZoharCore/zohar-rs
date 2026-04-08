use bevy::prelude::*;
use zohar_domain::entity::MovementAnimation;

use super::action_pipeline::{
    Action, apply_action, build_player_attack_action, build_player_move_action,
};
use super::aggro::{MobAggroDispatch, MobAggroDispatchBuffer};
use super::query::validate_player_attack;
use super::state::{
    LocalTransform, MapPendingMovementAnimations, MapSpatial, MobAggro, MobRef, NetEntityId,
    PlayerCommand, PlayerCommandQueue, PlayerMarker, PlayerMovementAnimation, RuntimeState,
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
                    if set_player_movement_animation(world, map_entity, player_entity, animation) {
                        queue_player_movement_animation(
                            world,
                            map_entity,
                            attacker_net_id,
                            animation,
                        );
                    }
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

                    if let Some(action) =
                        build_player_attack_action(world, player_entity, target, attack)
                    {
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
    _map_entity: Entity,
    player_entity: Entity,
    animation: MovementAnimation,
) -> bool {
    let mut player_entity_ref = world.entity_mut(player_entity);
    let Some(mut current) = player_entity_ref.get_mut::<PlayerMovementAnimation>() else {
        return false;
    };
    if current.0 == animation {
        return false;
    }
    current.0 = animation;
    true
}

fn queue_player_movement_animation(
    world: &mut World,
    map_entity: Entity,
    entity_id: zohar_domain::entity::EntityId,
    animation: MovementAnimation,
) {
    let mut map_entity_ref = world.entity_mut(map_entity);
    let Some(mut pending) = map_entity_ref.get_mut::<MapPendingMovementAnimations>() else {
        return;
    };
    pending.0.push(super::state::PendingMovementAnimation {
        entity_id,
        animation,
    });
}
