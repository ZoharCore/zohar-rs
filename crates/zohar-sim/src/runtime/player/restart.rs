use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::MovementAnimation;
use zohar_gameplay::stats::game::Stat;
use zohar_map_port::{ChatChannel, PlayerEvent, PlayerRestartIntent};

use crate::runtime::fanout::{ActorAudience, broadcast_actor_event, push_reliable};
use crate::runtime::spawn_payload::make_entity_snapshot;

use super::persistence::mark_player_dirty;
use super::state::{
    ActorLifeComp, LocalTransform, MapReplication, MapSpatial, NetEntityId, NetEntityIndex,
    PlayerActivityComp, PlayerMarker, PlayerMotion, PlayerMovementAnimation,
    PlayerRestartIntentQueue, PlayerStatTickerComp, PlayerStatsComp, PlayerTargetComp,
    RestartReadiness, RuntimeState, SharedConfig, SimDuration, SimInstant,
};

pub(crate) fn process_player_restarts(world: &mut World) {
    let now = world.resource::<RuntimeState>().sim_now;

    let requested = drain_restart_intents(world);
    for (player_entity, intent) in requested {
        process_requested_restart(world, player_entity, intent, now);
    }

    let forced = forced_respawn_players(world, now);
    for player_entity in forced {
        restart_town(world, player_entity, RestartHp::HalfMax);
    }
}

fn drain_restart_intents(world: &mut World) -> Vec<(Entity, PlayerRestartIntent)> {
    let mut query = world.query::<(Entity, &mut PlayerRestartIntentQueue)>();
    query
        .iter_mut(world)
        .filter_map(|(entity, mut queue)| {
            let intent = queue.0.pop()?;
            queue.0.clear();
            Some((entity, intent))
        })
        .collect()
}

fn forced_respawn_players(world: &mut World, now: SimInstant) -> Vec<Entity> {
    let mut query = world.query_filtered::<(Entity, &ActorLifeComp), With<PlayerMarker>>();
    query
        .iter(world)
        .filter_map(|(entity, life)| life.forced_respawn_due(now).then_some(entity))
        .collect()
}

fn process_requested_restart(
    world: &mut World,
    player_entity: Entity,
    intent: PlayerRestartIntent,
    now: SimInstant,
) {
    let Some(life) = world.entity(player_entity).get::<ActorLifeComp>().copied() else {
        return;
    };

    if !life.is_dead() {
        close_restart_window(world, player_entity);
        return;
    }

    match restart_readiness_for(life, intent, now) {
        Some(RestartReadiness::Ready) => {
            const MANUAL_RESTART_HP: i32 = 50;
            restart_by_intent(
                world,
                player_entity,
                intent,
                RestartHp::Fixed(MANUAL_RESTART_HP),
            );
        }
        Some(RestartReadiness::Waiting { retry_after }) => {
            push_restart_cooldown_feedback(world, player_entity, intent, retry_after);
        }
        None => {}
    }
}

#[derive(Clone, Copy)]
enum RestartHp {
    Fixed(i32),
    HalfMax,
}

#[derive(Clone, Copy)]
enum RestartVisibility {
    Here,
    TownHandoff,
}

#[derive(Clone, Copy)]
enum RestartPlacement {
    Current,
    MoveTo(LocalPos),
}

fn restart_readiness_for(
    life: ActorLifeComp,
    intent: PlayerRestartIntent,
    now: SimInstant,
) -> Option<RestartReadiness> {
    match intent {
        PlayerRestartIntent::Here => life.restart_here_readiness(now),
        PlayerRestartIntent::Town => life.restart_town_readiness(now),
    }
}

fn restart_by_intent(
    world: &mut World,
    player_entity: Entity,
    intent: PlayerRestartIntent,
    hp: RestartHp,
) {
    match intent {
        PlayerRestartIntent::Here => restart_here(world, player_entity, hp),
        PlayerRestartIntent::Town => restart_town(world, player_entity, hp),
    }
}

fn restart_here(world: &mut World, player_entity: Entity, hp: RestartHp) {
    if let Some(pos) = world
        .entity(player_entity)
        .get::<LocalTransform>()
        .map(|transform| transform.pos)
    {
        restart_player(
            world,
            player_entity,
            RestartPlacement::MoveTo(pos),
            hp,
            RestartVisibility::Here,
        );
    }
}

fn restart_town(world: &mut World, player_entity: Entity, hp: RestartHp) {
    restart_player(
        world,
        player_entity,
        RestartPlacement::Current,
        hp,
        RestartVisibility::TownHandoff,
    );
}

fn restart_player(
    world: &mut World,
    player_entity: Entity,
    placement: RestartPlacement,
    hp: RestartHp,
    visibility: RestartVisibility,
) {
    let Some(net_id) = world
        .entity(player_entity)
        .get::<NetEntityId>()
        .map(|net| net.net_id)
    else {
        return;
    };

    apply_restart_state(world, player_entity, net_id, placement, hp);
    close_restart_window(world, player_entity);

    match visibility {
        RestartVisibility::Here => {
            let shared = world.resource::<SharedConfig>().clone();
            let Some(snapshot) = make_entity_snapshot(world, &shared, net_id) else {
                return;
            };

            // Observers: simple despawn + respawn of the player.
            broadcast_actor_event(world, net_id, ActorAudience::Observers, |id| {
                PlayerEvent::EntityDespawn { entity_id: id }
            });
            broadcast_actor_event(world, net_id, ActorAudience::Observers, |_| {
                PlayerEvent::EntitySpawn {
                    snapshot: snapshot.clone(),
                }
            });

            // Self: full refresh required because client purges all entities.
            push_reliable(
                world,
                player_entity,
                PlayerEvent::EntityDespawn { entity_id: net_id },
            );
            push_spawn_bundle(world, player_entity, snapshot);

            // Re-spawn all currently visible nearby entities for the player.
            let nearby = current_targets(world, net_id);
            for target_id in nearby {
                let Some(target_snapshot) = make_entity_snapshot(world, &shared, target_id) else {
                    continue;
                };
                push_spawn_bundle(world, player_entity, target_snapshot);
            }
        }

        RestartVisibility::TownHandoff => {
            push_reliable(world, player_entity, PlayerEvent::RestartTown);
        }
    }

    world.resource_mut::<RuntimeState>().is_dirty = true;
}

fn apply_restart_state(
    world: &mut World,
    player_entity: Entity,
    net_id: EntityId,
    placement: RestartPlacement,
    hp: RestartHp,
) {
    let sim_now = world.resource::<RuntimeState>().sim_now;
    let packet_now = world.resource::<RuntimeState>().packet_now();
    let current_pos = world
        .entity(player_entity)
        .get::<LocalTransform>()
        .map(|transform| transform.pos);
    let motion_pos = match placement {
        RestartPlacement::Current => current_pos,
        RestartPlacement::MoveTo(pos) => Some(pos),
    };

    {
        let mut player = world.entity_mut(player_entity);
        if let Some(mut life) = player.get_mut::<ActorLifeComp>() {
            let _ = life.restart();
        }
        if let RestartPlacement::MoveTo(pos) = placement
            && let Some(mut transform) = player.get_mut::<LocalTransform>()
        {
            transform.pos = pos;
        }
        if let Some(pos) = motion_pos
            && let Some(mut motion) = player.get_mut::<PlayerMotion>()
        {
            motion.0.segment_start_pos = pos;
            motion.0.segment_end_pos = pos;
            motion.0.segment_start_ts = packet_now;
            motion.0.segment_end_ts = packet_now;
            motion.0.last_client_ts = packet_now;
        }
        if let Some(mut animation) = player.get_mut::<PlayerMovementAnimation>() {
            animation.0 = MovementAnimation::default();
        }
        if let Some(mut activity) = player.get_mut::<PlayerActivityComp>() {
            *activity = PlayerActivityComp::default();
        }
        if let Some(mut tickers) = player.get_mut::<PlayerStatTickerComp>() {
            tickers.reset_after_restart(sim_now);
        }
        if let Some(mut target) = player.get_mut::<PlayerTargetComp>() {
            target.selected = None;
        }
        if let Some(mut stats) = player.get_mut::<PlayerStatsComp>() {
            let hp = match hp {
                RestartHp::Fixed(value) => value,
                RestartHp::HalfMax => stats.0.read_packet(Stat::MaxHp) / 2,
            };
            let _ = stats.0.with_api_mut(|api| api.set_resource(Stat::Hp, hp));
        }
    }

    if let RestartPlacement::MoveTo(pos) = placement
        && let Some(map_entity) = world.resource::<RuntimeState>().map_entity
        && let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>()
    {
        spatial.0.update_position(net_id, pos);
        spatial.0.maintain();
    }

    mark_player_dirty(world, player_entity);
}

fn current_targets(world: &World, observer: EntityId) -> Vec<EntityId> {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return Vec::new();
    };
    world
        .entity(map_entity)
        .get::<MapReplication>()
        .map(|replication| replication.0.targets_for(observer))
        .unwrap_or_default()
}

fn push_spawn_bundle(
    world: &mut World,
    player_entity: Entity,
    snapshot: zohar_domain::appearance::EntitySnapshot,
) {
    let entity_id = snapshot.entity_id;
    let movement_animation = world
        .resource::<NetEntityIndex>()
        .0
        .get(&entity_id)
        .and_then(|entity| {
            world
                .entity(*entity)
                .get::<PlayerMovementAnimation>()
                .map(|animation| animation.0)
        });

    push_reliable(world, player_entity, PlayerEvent::EntitySpawn { snapshot });
    if movement_animation == Some(MovementAnimation::Walk) {
        push_reliable(
            world,
            player_entity,
            PlayerEvent::SetEntityMovementAnimation {
                entity_id,
                animation: MovementAnimation::Walk,
            },
        );
    }
}

fn push_restart_cooldown_feedback(
    world: &mut World,
    player_entity: Entity,
    intent: PlayerRestartIntent,
    retry_after: SimDuration,
) {
    let seconds = wait_seconds(retry_after);
    let message = match intent {
        PlayerRestartIntent::Here => {
            format!("A new start is not possible at the moment. Please wait {seconds} seconds.")
        }
        PlayerRestartIntent::Town => {
            format!("You cannot restart in the city yet. Wait another {seconds} seconds.")
        }
    };
    push_chat(
        world,
        player_entity,
        ChatChannel::Info,
        message.into_bytes(),
    );
}

fn close_restart_window(world: &mut World, player_entity: Entity) {
    push_chat(
        world,
        player_entity,
        ChatChannel::Command,
        b"CloseRestartWindow".to_vec(),
    );
}

fn push_chat(world: &mut World, player_entity: Entity, channel: ChatChannel, mut message: Vec<u8>) {
    message.push(0);
    push_reliable(
        world,
        player_entity,
        PlayerEvent::Chat {
            channel,
            message,
            sender_entity_id: None,
            empire: None,
        },
    );
}

fn wait_seconds(wait: SimDuration) -> u64 {
    wait.as_millis().div_ceil(1_000).max(1)
}
