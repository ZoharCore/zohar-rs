use bevy::prelude::*;
use zohar_domain::entity::player::PlayerId;

use crate::api::PlayerEvent;
use crate::bridge::{EnterMsg, LeaveMsg};

use super::state::{
    ChatIntentQueue, LocalTransform, MapPendingLocalChats, MapPendingMovements, MapReplication,
    MapSpatial, MoveIntentQueue, NetEntityId, NetEntityIndex, PlayerAppearanceComp, PlayerCount,
    PlayerIndex, PlayerMarker, PlayerMotion, PlayerMotionState, PlayerOutboxComp, RuntimeState,
};
use tracing::{info, warn};

pub(super) fn on_player_added(
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

pub(super) fn on_player_removed(
    remove: On<Remove, PlayerMarker>,
    player_query: Query<(&PlayerMarker, &NetEntityId)>,
    mut map_query: Query<(
        &mut MapSpatial,
        &mut MapReplication,
        &mut MapPendingLocalChats,
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
        && let Ok((mut spatial, mut replication, mut pending_chats, mut pending)) =
            map_query.get_mut(map_entity)
    {
        let _ = replication.0.remove_observer(net_id.net_id);
        let observers = replication.0.remove_target(net_id.net_id);
        spatial.0.remove(net_id.net_id);

        pending_chats.0.retain(|chat| {
            chat.speaker_player_id != marker.player_id && chat.speaker_entity_id != net_id.net_id
        });
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

pub(super) fn handle_player_enter(world: &mut World, mut msg: EnterMsg) {
    if let Some(existing_entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    {
        let _ = world.despawn(existing_entity);
    }

    msg.outbox.set_owner_player_id(msg.player_id);
    world.spawn((
        PlayerMarker {
            player_id: msg.player_id,
        },
        NetEntityId {
            net_id: msg.player_net_id,
        },
        LocalTransform {
            pos: msg.initial_pos,
            rot: 0,
        },
        PlayerMotion(PlayerMotionState {
            segment_start_pos: msg.initial_pos,
            segment_end_pos: msg.initial_pos,
            segment_start_ts: 0,
            segment_end_ts: 0,
            last_client_ts: 0,
        }),
        PlayerAppearanceComp(msg.appearance.clone()),
        PlayerOutboxComp(msg.outbox),
        MoveIntentQueue::default(),
        ChatIntentQueue::default(),
    ));

    world.resource_mut::<RuntimeState>().is_dirty = true;
}

pub(super) fn handle_player_leave(world: &mut World, msg: LeaveMsg) {
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

pub(super) fn player_entities_on_map(world: &mut World) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &PlayerMarker)>();
    query.iter(world).map(|(entity, _)| entity).collect()
}

#[allow(dead_code)]
pub(super) fn map_has_players(world: &mut World) -> bool {
    let mut query = world.query::<&PlayerMarker>();
    query.iter(world).next().is_some()
}

#[allow(dead_code)]
pub(super) fn player_entity_for_id(world: &mut World, player_id: PlayerId) -> Option<Entity> {
    world.resource::<PlayerIndex>().0.get(&player_id).copied()
}
