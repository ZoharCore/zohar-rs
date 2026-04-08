use bevy::prelude::*;
use zohar_domain::entity::player::PlayerId;
use zohar_map_port::{ClientTimestamp, EnterMsg, Facing72, LeaveMsg, PlayerEvent};

use crate::outbox::PlayerOutbox;
use crate::runtime::net::replication::bootstrap_observer_snapshot;
use crate::runtime::spawn_events::make_player_spawn_payload;

use super::state::{
    ChatIntentQueue, LocalTransform, MapPendingLocalChats, MapPendingMovementAnimations,
    MapPendingMovements, MapReplication, MapSpatial, NetEntityId, NetEntityIndex,
    PlayerAppearanceComp, PlayerCommandQueue, PlayerCount, PlayerIndex, PlayerMarker, PlayerMotion,
    PlayerMotionState, PlayerMovementAnimation, PlayerOutboxComp, RuntimeState,
};
use tracing::{info, warn};

use super::PlayerPersistenceState;

pub(crate) fn on_player_added(
    add: On<Add, PlayerMarker>,
    player_query: Query<(&PlayerMarker, &NetEntityId, &LocalTransform)>,
    mut map_query: Query<&mut MapSpatial>,
    mut state: ResMut<RuntimeState>,
    mut player_index: ResMut<PlayerIndex>,
    mut net_index: ResMut<NetEntityIndex>,
    mut player_count: ResMut<PlayerCount>,
) {
    let entity = add.entity;
    let Ok((marker, net_id, transform)) = player_query.get(entity) else {
        return;
    };

    player_index.0.insert(marker.player_id, entity);
    net_index.0.insert(net_id.net_id, entity);

    player_count.0 = player_count.0.saturating_add(1);

    if let Some(map_entity) = state.map_entity
        && let Ok(mut spatial) = map_query.get_mut(map_entity)
    {
        spatial.0.insert(net_id.net_id, transform.pos);
    }

    state.is_dirty = true;
    info!(
        player_id = ?marker.player_id,
        net_id = ?net_id.net_id,
        map_players = player_count.0,
        "Player added to map runtime"
    );
}

pub(crate) fn on_player_removed(
    remove: On<Remove, PlayerMarker>,
    player_query: Query<(&PlayerMarker, &NetEntityId)>,
    mut map_query: Query<(
        &mut MapSpatial,
        &mut MapReplication,
        &mut MapPendingLocalChats,
        &mut MapPendingMovementAnimations,
        &mut MapPendingMovements,
    )>,
    mut outbox_query: Query<(Entity, &NetEntityId, &mut PlayerOutboxComp), With<PlayerMarker>>,
    mut state: ResMut<RuntimeState>,
    mut player_index: ResMut<PlayerIndex>,
    mut net_index: ResMut<NetEntityIndex>,
    mut player_count: ResMut<PlayerCount>,
) {
    let entity = remove.entity;
    let Ok((marker, net_id)) = player_query.get(entity) else {
        return;
    };

    if let Some(map_entity) = state.map_entity
        && let Ok((
            mut spatial,
            mut replication,
            mut pending_chats,
            mut pending_animations,
            mut pending,
        )) = map_query.get_mut(map_entity)
    {
        let _ = replication.0.remove_observer(net_id.net_id);
        let observers = replication.0.remove_target(net_id.net_id);
        spatial.0.remove(net_id.net_id);

        pending_chats.0.retain(|chat| {
            chat.speaker_player_id != marker.player_id && chat.speaker_entity_id != net_id.net_id
        });
        pending_animations
            .0
            .retain(|animation| animation.entity_id != net_id.net_id);
        pending.0.retain(|movement| {
            movement.entity_id != net_id.net_id
                && movement.mover_player_id != Some(marker.player_id)
        });

        for observer_net in observers {
            if let Some(observer_entity) = net_index.0.get(&observer_net).copied()
                && observer_entity != entity
                && let Ok((_, _, mut observer_outbox)) = outbox_query.get_mut(observer_entity)
            {
                observer_outbox.0.push_reliable(PlayerEvent::EntityDespawn {
                    entity_id: net_id.net_id,
                });
            }
        }
    }

    player_index.0.remove(&marker.player_id);
    net_index.0.remove(&net_id.net_id);
    player_count.0 = player_count.0.saturating_sub(1);
    state.is_dirty = player_count.0 > 0;
    info!(
        player_id = ?marker.player_id,
        net_id = ?net_id.net_id,
        map_players = player_count.0,
        "Player removed from map runtime"
    );
}

pub(crate) fn handle_player_enter(world: &mut World, msg: EnterMsg, mut outbox: PlayerOutbox) {
    let now = world.resource::<RuntimeState>().sim_now;

    if let Some(existing_entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    {
        let _ = world.despawn(existing_entity);
    }

    let initial_rot = Facing72::from_wrapped(0);
    let (show, details) = make_player_spawn_payload(
        msg.player_net_id,
        msg.initial_pos,
        initial_rot,
        &msg.appearance,
    );

    outbox.set_owner_player_id(msg.player_id);
    outbox.push_reliable(PlayerEvent::EntitySpawn {
        show,
        details: Some(details),
    });

    let player_entity = world
        .spawn((
            PlayerMarker {
                player_id: msg.player_id,
            },
            NetEntityId {
                net_id: msg.player_net_id,
            },
            LocalTransform {
                pos: msg.initial_pos,
                rot: initial_rot,
            },
            PlayerMotion(PlayerMotionState {
                segment_start_pos: msg.initial_pos,
                segment_end_pos: msg.initial_pos,
                segment_start_ts: ClientTimestamp::ZERO,
                segment_end_ts: ClientTimestamp::ZERO,
                last_client_ts: ClientTimestamp::ZERO,
            }),
            PlayerAppearanceComp(msg.appearance.clone()),
            PlayerMovementAnimation::default(),
            PlayerOutboxComp(outbox),
            PlayerCommandQueue::default(),
            ChatIntentQueue::default(),
            PlayerPersistenceState::initial(msg.player_id, msg.runtime_epoch, now),
        ))
        .id();

    bootstrap_observer_snapshot(world, msg.player_id, msg.player_net_id, msg.initial_pos);

    if let Some(mut outbox) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerOutboxComp>()
    {
        let _ = outbox.0.flush();
    }

    world.resource_mut::<RuntimeState>().is_dirty = true;
}

pub(crate) fn handle_player_leave(world: &mut World, msg: LeaveMsg) {
    let Some(entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    else {
        return;
    };

    let Some(current_net_id) = world.entity(entity).get::<NetEntityId>().map(|n| n.net_id) else {
        return;
    };

    if current_net_id != msg.player_net_id {
        warn!(
            player_id = ?msg.player_id,
            expected_net_id = ?current_net_id,
            leave_net_id = ?msg.player_net_id,
            "Ignoring player leave with mismatched net id"
        );
        return;
    }

    let _ = world.despawn(entity);
}

pub(crate) fn player_entities_on_map(world: &mut World) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &PlayerMarker)>();
    query.iter(world).map(|(entity, _)| entity).collect()
}

#[allow(dead_code)]
pub(crate) fn map_has_players(world: &mut World) -> bool {
    let mut query = world.query::<&PlayerMarker>();
    query.iter(world).next().is_some()
}

#[allow(dead_code)]
pub(crate) fn player_entity_for_id(world: &mut World, player_id: PlayerId) -> Option<Entity> {
    world.resource::<PlayerIndex>().0.get(&player_id).copied()
}
