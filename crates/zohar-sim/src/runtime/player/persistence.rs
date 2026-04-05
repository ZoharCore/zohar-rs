use anyhow::anyhow;
use bevy::prelude::*;
use tracing::warn;
use zohar_domain::entity::player::{PlayerId, PlayerRuntimeSnapshot};
use zohar_map_port::LeaveMsg;

use crate::persistence::PlayerPersistencePort;
use crate::runtime::spatial::sample_player_visual_position_at;

use super::super::common::{LocalTransform, MapConfig, PlayerIndex, RuntimeState};
use super::super::time::{SimDuration, SimInstant};
use super::{PlayerMarker, PlayerMotion, PlayerPersistenceState};

const AUTOSAVE_INTERVAL: SimDuration = SimDuration::from_millis(30_000);
const AUTOSAVE_RETRY_DELAY: SimDuration = SimDuration::from_millis(1_000);

impl PlayerPersistenceState {
    pub(crate) fn initial(player_id: PlayerId, now: SimInstant) -> Self {
        Self {
            dirty: false,
            next_autosave_at: Self::initial_autosave_deadline(player_id, now),
        }
    }

    fn initial_autosave_deadline(player_id: PlayerId, now: SimInstant) -> SimInstant {
        let interval_ms = AUTOSAVE_INTERVAL.as_millis();
        let phase_ms = i64::from(player_id).unsigned_abs() % interval_ms;
        let now_ms = u64::from(now);
        let now_phase = now_ms % interval_ms;
        let delay_ms = if phase_ms > now_phase {
            phase_ms - now_phase
        } else {
            interval_ms - (now_phase - phase_ms)
        };

        now.saturating_add(SimDuration::from_millis(delay_ms.max(1)))
    }
}

pub(crate) fn mark_player_dirty(world: &mut World, player_entity: Entity) {
    let mut player = world.entity_mut(player_entity);
    let Some(mut persistence) = player.get_mut::<PlayerPersistenceState>() else {
        return;
    };
    persistence.dirty = true;
}

pub(crate) fn enqueue_due_autosaves(
    map: Res<MapConfig>,
    persistence_port: Res<PlayerPersistencePort>,
    state: Res<RuntimeState>,
    mut query: Query<(
        &PlayerMarker,
        &LocalTransform,
        Option<&PlayerMotion>,
        &mut PlayerPersistenceState,
    )>,
) {
    let now = state.sim_now;

    for (marker, transform, motion, mut persistence) in &mut query {
        if persistence.next_autosave_at > now {
            continue;
        }

        if !persistence.dirty {
            advance_autosave_deadline(&mut persistence, now);
            continue;
        }

        let snapshot = PlayerRuntimeSnapshot {
            id: marker.player_id,
            map_key: map.map_code.clone(),
            local_pos: snapshot_local_pos(&map, &state, transform, motion.map(|motion| motion.0)),
        };

        match persistence_port.handle().try_schedule_autosave(snapshot) {
            Ok(()) => {
                persistence.dirty = false;
                advance_autosave_deadline(&mut persistence, now);
            }
            Err(error) => {
                warn!(
                    player_id = ?marker.player_id,
                    error = %error,
                    "Failed to enqueue player autosave"
                );
                persistence.next_autosave_at = now.saturating_add(AUTOSAVE_RETRY_DELAY);
            }
        }
    }
}

pub(crate) fn leave_player_and_snapshot(
    world: &mut World,
    msg: LeaveMsg,
) -> anyhow::Result<PlayerRuntimeSnapshot> {
    let Some(entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    else {
        return Err(anyhow!(
            "player {:?} is not active in the map runtime",
            msg.player_id
        ));
    };

    let Some(current_net_id) = world
        .entity(entity)
        .get::<super::super::state::NetEntityId>()
    else {
        return Err(anyhow!(
            "player {:?} is missing a runtime net entity id",
            msg.player_id
        ));
    };

    if current_net_id.net_id != msg.player_net_id {
        return Err(anyhow!(
            "player {:?} leave request used stale net id {:?} (expected {:?})",
            msg.player_id,
            msg.player_net_id,
            current_net_id.net_id
        ));
    }

    let Some(transform) = world.entity(entity).get::<LocalTransform>() else {
        return Err(anyhow!(
            "player {:?} is missing a local transform",
            msg.player_id
        ));
    };
    let map_code = world.resource::<MapConfig>().map_code.clone();
    let snapshot_pos = snapshot_local_pos(
        world.resource::<MapConfig>(),
        world.resource::<RuntimeState>(),
        transform,
        world
            .entity(entity)
            .get::<PlayerMotion>()
            .map(|motion| motion.0),
    );
    let snapshot = PlayerRuntimeSnapshot {
        id: msg.player_id,
        map_key: map_code,
        local_pos: snapshot_pos,
    };
    let _ = world.despawn(entity);
    Ok(snapshot)
}

fn advance_autosave_deadline(persistence: &mut PlayerPersistenceState, now: SimInstant) {
    while persistence.next_autosave_at <= now {
        persistence.next_autosave_at = persistence
            .next_autosave_at
            .saturating_add(AUTOSAVE_INTERVAL);
    }
}

fn snapshot_local_pos(
    map: &MapConfig,
    state: &RuntimeState,
    transform: &LocalTransform,
    motion: Option<super::PlayerMotionState>,
) -> zohar_domain::coords::LocalPos {
    let Some(motion) = motion else {
        return transform.pos;
    };

    let sampled = sample_player_visual_position_at(motion, state.packet_now());

    // clip to a valid cell if sampled position is blocked by the collision grid
    if let Some(navigator) = map.navigator.as_deref()
        && !navigator.can_stand(sampled)
    {
        let clipped = navigator.clip_segment(motion.segment_start_pos, sampled);
        if navigator.can_stand(clipped) {
            return clipped;
        }
    }

    sampled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::EntityMotionSpeedTable;
    use crate::navigation::{MapNavigator, TerrainFlagsGrid};
    use crate::persistence::{
        PlayerPersistencePort, PlayerPersistenceRequest, SnapshotSaveKind,
        player_persistence_channel,
    };
    use crate::runtime::common::NetEntityId;
    use crate::runtime::player::PlayerMotionState;
    use crate::runtime::time::SimInstant;
    use crate::types::MapInstanceKey;
    use crate::{MapConfig, SharedConfig, WanderConfig};
    use std::collections::HashMap;
    use std::sync::Arc;
    use zohar_domain::MapId;
    use zohar_domain::TerrainFlags;
    use zohar_domain::coords::{LocalPos, LocalSize};
    use zohar_domain::entity::EntityId;
    use zohar_map_port::ClientTimestamp;
    use zohar_map_port::Facing72;

    fn test_shared_config() -> SharedConfig {
        SharedConfig {
            motion_speeds: Arc::new(EntityMotionSpeedTable::default()),
            mobs: Arc::new(HashMap::new()),
            wander: WanderConfig::default(),
            mob_chat: Arc::default(),
        }
    }

    fn test_map_config(navigator: Option<Arc<MapNavigator>>) -> MapConfig {
        MapConfig {
            map_key: MapInstanceKey::shared(1, MapId::new(1)),
            map_code: "zohar_map_a1".to_string(),
            empire: None,
            local_size: LocalSize::new(16_384.0, 16_384.0),
            navigator,
            spawn_rules: Vec::new(),
        }
    }

    fn test_navigator(
        width: usize,
        height: usize,
        blocked_cells: &[(usize, usize)],
    ) -> Arc<MapNavigator> {
        let mut flags = vec![TerrainFlags::empty(); width * height];
        for (x, y) in blocked_cells.iter().copied() {
            flags[y * width + x] = TerrainFlags::BLOCK;
        }
        Arc::new(MapNavigator::new(
            TerrainFlagsGrid::new(1.0, width, height, flags).expect("terrain flags grid"),
        ))
    }

    #[test]
    fn initial_autosave_deadline_is_stable_and_bounded() {
        let now = SimInstant::from_millis(5_000);
        let first = PlayerPersistenceState::initial_autosave_deadline(PlayerId::from(42), now);
        let second = PlayerPersistenceState::initial_autosave_deadline(PlayerId::from(42), now);

        assert_eq!(first, second);

        let delay_ms = u64::from(first.saturating_sub(now));
        assert!(delay_ms > 0);
        assert!(delay_ms <= AUTOSAVE_INTERVAL.as_millis());
    }

    #[test]
    fn enqueue_due_autosaves_enqueues_dirty_players_and_clears_dirty_state() {
        let (handle, mut rx) = player_persistence_channel(4);
        let mut app = App::new();
        app.insert_resource(test_map_config(None));
        app.insert_resource(test_shared_config());
        app.insert_resource(PlayerPersistencePort::new(handle));
        app.insert_resource(RuntimeState {
            sim_now: SimInstant::from_millis(30_000),
            ..Default::default()
        });
        app.add_systems(Update, enqueue_due_autosaves);

        let player_id = PlayerId::from(1);
        let entity = app
            .world_mut()
            .spawn((
                PlayerMarker { player_id },
                LocalTransform {
                    pos: LocalPos::new(11.0, 22.0),
                    rot: Facing72::from_wrapped(0),
                },
                PlayerPersistenceState {
                    dirty: true,
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();

        app.update();

        let request = rx.try_recv().expect("autosave request");
        match request {
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot,
                kind,
                reply,
            } => {
                assert_eq!(snapshot.id, player_id);
                assert!(matches!(kind, SnapshotSaveKind::Autosave));
                assert!(reply.is_none());
                assert_eq!(snapshot.map_key, "zohar_map_a1");
                assert_eq!(snapshot.local_pos, LocalPos::new(11.0, 22.0));
            }
            other => panic!("unexpected request: {other:?}"),
        }

        let persistence = app
            .world()
            .entity(entity)
            .get::<PlayerPersistenceState>()
            .expect("persistence state");
        assert!(!persistence.dirty);
        assert!(persistence.next_autosave_at > SimInstant::from_millis(30_000));
    }

    #[test]
    fn enqueue_due_autosaves_samples_inflight_player_motion() {
        let (handle, mut rx) = player_persistence_channel(4);
        let mut app = App::new();
        app.insert_resource(test_map_config(None));
        app.insert_resource(test_shared_config());
        app.insert_resource(PlayerPersistencePort::new(handle));
        app.insert_resource(RuntimeState {
            sim_now: SimInstant::from_millis(3_000),
            ..Default::default()
        });
        app.add_systems(Update, enqueue_due_autosaves);

        let player_id = PlayerId::from(1);
        app.world_mut().spawn((
            PlayerMarker { player_id },
            LocalTransform {
                pos: LocalPos::new(8.0, 4.0),
                rot: Facing72::from_wrapped(0),
            },
            PlayerMotion(PlayerMotionState {
                segment_start_pos: LocalPos::new(4.0, 4.0),
                segment_end_pos: LocalPos::new(8.0, 4.0),
                segment_start_ts: ClientTimestamp::new(1_000),
                segment_end_ts: ClientTimestamp::new(5_000),
                last_client_ts: ClientTimestamp::new(1_000),
            }),
            PlayerPersistenceState {
                dirty: true,
                next_autosave_at: SimInstant::ZERO,
            },
        ));

        app.update();

        let request = rx.try_recv().expect("autosave request");
        match request {
            PlayerPersistenceRequest::SaveSnapshot { snapshot, .. } => {
                assert_eq!(snapshot.local_pos, LocalPos::new(6.0, 4.0));
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn leave_player_and_snapshot_despawns_player_and_returns_snapshot() {
        let mut world = World::new();
        world.insert_resource(test_map_config(None));
        world.insert_resource(RuntimeState::default());
        world.insert_resource(PlayerIndex::default());

        let player_id = PlayerId::from(7);
        let entity = world
            .spawn((
                PlayerMarker { player_id },
                NetEntityId {
                    net_id: EntityId(77),
                },
                LocalTransform {
                    pos: LocalPos::new(4.0, 9.0),
                    rot: Facing72::from_wrapped(0),
                },
                PlayerPersistenceState {
                    dirty: true,
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();
        world
            .resource_mut::<PlayerIndex>()
            .0
            .insert(player_id, entity);

        let snapshot = leave_player_and_snapshot(
            &mut world,
            LeaveMsg {
                player_id,
                player_net_id: EntityId(77),
            },
        )
        .expect("snapshot");

        assert!(!world.entities().contains(entity));
        assert_eq!(snapshot.id, player_id);
        assert_eq!(snapshot.local_pos, LocalPos::new(4.0, 9.0));
    }

    #[test]
    fn leave_player_and_snapshot_clips_sampled_position_before_entering_blocked_cell() {
        let mut world = World::new();
        let navigator = test_navigator(6, 4, &[(2, 1), (3, 1), (4, 1)]);
        world.insert_resource(test_map_config(Some(Arc::clone(&navigator))));
        world.insert_resource(RuntimeState {
            sim_now: SimInstant::from_millis(500),
            ..Default::default()
        });
        world.insert_resource(PlayerIndex::default());

        let player_id = PlayerId::from(7);
        let entity = world
            .spawn((
                PlayerMarker { player_id },
                NetEntityId {
                    net_id: EntityId(77),
                },
                LocalTransform {
                    pos: LocalPos::new(3.2, 1.2),
                    rot: Facing72::from_wrapped(0),
                },
                PlayerMotion(PlayerMotionState {
                    segment_start_pos: LocalPos::new(1.2, 1.2),
                    segment_end_pos: LocalPos::new(3.2, 1.2),
                    segment_start_ts: ClientTimestamp::new(0),
                    segment_end_ts: ClientTimestamp::new(1_000),
                    last_client_ts: ClientTimestamp::new(0),
                }),
                PlayerPersistenceState {
                    dirty: true,
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();
        world
            .resource_mut::<PlayerIndex>()
            .0
            .insert(player_id, entity);

        let snapshot = leave_player_and_snapshot(
            &mut world,
            LeaveMsg {
                player_id,
                player_net_id: EntityId(77),
            },
        )
        .expect("snapshot");

        assert!(!world.entities().contains(entity));
        assert!(navigator.can_stand(snapshot.local_pos));
        assert!(snapshot.local_pos.x < 2.0);
        assert!(snapshot.local_pos.x > 1.9);
        assert_eq!(snapshot.local_pos.y, 1.2);
    }

    #[test]
    fn leave_player_and_snapshot_rejects_stale_net_id() {
        let mut world = World::new();
        world.insert_resource(test_map_config(None));
        world.insert_resource(RuntimeState::default());
        world.insert_resource(PlayerIndex::default());

        let player_id = PlayerId::from(7);
        let entity = world
            .spawn((
                PlayerMarker { player_id },
                NetEntityId {
                    net_id: EntityId(77),
                },
                LocalTransform {
                    pos: LocalPos::new(4.0, 9.0),
                    rot: Facing72::from_wrapped(0),
                },
                PlayerPersistenceState {
                    dirty: true,
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();
        world
            .resource_mut::<PlayerIndex>()
            .0
            .insert(player_id, entity);

        let error = leave_player_and_snapshot(
            &mut world,
            LeaveMsg {
                player_id,
                player_net_id: EntityId(78),
            },
        )
        .expect_err("snapshot should fail");
        assert!(error.to_string().contains("stale net id"));
        assert!(world.entities().contains(entity));
    }
}
