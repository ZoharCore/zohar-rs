use anyhow::anyhow;
use bevy::prelude::*;
use std::time::Duration;
use tracing::warn;
use zohar_domain::entity::player::{
    PlayerId, PlayerPlaytime, PlayerProgressionSnapshot, PlayerRuntimeEpoch, PlayerRuntimeSnapshot,
    PlayerSnapshot,
};
use zohar_gameplay::stats::game::{GameStatsApi, Stat};
use zohar_map_port::LeaveMsg;

use crate::persistence::PlayerPersistencePort;
use crate::runtime::spatial::sample_player_visual_position_at;

use super::super::common::{LocalTransform, MapConfig, PlayerIndex, RuntimeState};
use super::super::time::{SimDuration, SimInstant};
use super::{
    PlayerMarker, PlayerMotion, PlayerPendingDurableFlush, PlayerPersistenceState,
    PlayerProgressionComp, PlayerStatsComp,
};

const AUTOSAVE_INTERVAL: SimDuration = SimDuration::from_millis(30_000);
const AUTOSAVE_RETRY_DELAY: SimDuration = SimDuration::from_millis(1_000);

impl PlayerPersistenceState {
    pub(crate) fn initial(
        player_id: PlayerId,
        runtime_epoch: PlayerRuntimeEpoch,
        persisted_playtime: PlayerPlaytime,
        now: SimInstant,
    ) -> Self {
        Self {
            dirty: false,
            runtime_epoch,
            persisted_playtime,
            entered_map_at: now,
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

#[allow(clippy::type_complexity)]
pub(crate) fn enqueue_due_autosaves(
    map: Res<MapConfig>,
    persistence_port: Res<PlayerPersistencePort>,
    state: Res<RuntimeState>,
    mut query: Query<(
        &PlayerMarker,
        &LocalTransform,
        Option<&PlayerMotion>,
        &PlayerProgressionComp,
        &PlayerStatsComp,
        Option<&PlayerPendingDurableFlush>,
        &mut PlayerPersistenceState,
    )>,
) {
    let now = state.sim_now;

    for (marker, transform, motion, progression, stats, pending_flush, mut persistence) in
        &mut query
    {
        if persistence.next_autosave_at > now {
            continue;
        }

        if pending_flush.is_some_and(|pending| pending.0.is_some()) {
            continue;
        }

        if !persistence.dirty {
            advance_autosave_deadline(&mut persistence, now);
            continue;
        }

        let snapshot = player_snapshot(
            player_runtime_snapshot(
                &map,
                &state,
                marker.player_id,
                persistence.runtime_epoch,
                current_playtime(&persistence, now),
                transform,
                motion.map(|motion| motion.0),
                current_resources(stats),
            ),
            player_progression_snapshot(progression),
        );

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

pub(crate) fn capture_player_snapshot(
    world: &World,
    player_entity: Entity,
    progression: PlayerProgressionSnapshot,
) -> anyhow::Result<PlayerSnapshot> {
    let Some(marker) = world.entity(player_entity).get::<PlayerMarker>() else {
        return Err(anyhow!("player entity is missing player marker"));
    };
    let Some(transform) = world.entity(player_entity).get::<LocalTransform>() else {
        return Err(anyhow!("player entity is missing local transform"));
    };
    let Some(persistence) = world.entity(player_entity).get::<PlayerPersistenceState>() else {
        return Err(anyhow!("player entity is missing persistence state"));
    };
    let Some(stats) = world.entity(player_entity).get::<PlayerStatsComp>() else {
        return Err(anyhow!("player entity is missing stats state"));
    };

    let map = world.resource::<MapConfig>();
    let state = world.resource::<RuntimeState>();
    Ok(player_snapshot(
        player_runtime_snapshot(
            map,
            state,
            marker.player_id,
            persistence.runtime_epoch,
            current_playtime(persistence, state.sim_now),
            transform,
            world
                .entity(player_entity)
                .get::<PlayerMotion>()
                .map(|motion| motion.0),
            current_resources(stats),
        ),
        progression,
    ))
}

fn active_player_entity(world: &World, msg: &LeaveMsg) -> anyhow::Result<Entity> {
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

    Ok(entity)
}

pub(crate) fn capture_active_player_snapshot(
    world: &World,
    msg: LeaveMsg,
) -> anyhow::Result<PlayerSnapshot> {
    let entity = active_player_entity(world, &msg)?;
    let Some(progression) = world.entity(entity).get::<PlayerProgressionComp>() else {
        return Err(anyhow!(
            "player {:?} is missing progression state",
            msg.player_id
        ));
    };
    capture_player_snapshot(world, entity, player_progression_snapshot(progression))
}

pub(crate) fn leave_player_and_snapshot(
    world: &mut World,
    msg: LeaveMsg,
) -> anyhow::Result<PlayerSnapshot> {
    let entity = active_player_entity(world, &msg)?;

    let Some(transform) = world.entity(entity).get::<LocalTransform>() else {
        return Err(anyhow!(
            "player {:?} is missing a local transform",
            msg.player_id
        ));
    };
    let Some(persistence) = world.entity(entity).get::<PlayerPersistenceState>() else {
        return Err(anyhow!(
            "player {:?} is missing persistence state",
            msg.player_id
        ));
    };
    let Some(progression) = world.entity(entity).get::<PlayerProgressionComp>() else {
        return Err(anyhow!(
            "player {:?} is missing progression state",
            msg.player_id
        ));
    };
    let Some(stats) = world.entity(entity).get::<PlayerStatsComp>() else {
        return Err(anyhow!("player {:?} is missing stats state", msg.player_id));
    };
    let snapshot = player_snapshot(
        player_runtime_snapshot(
            world.resource::<MapConfig>(),
            world.resource::<RuntimeState>(),
            msg.player_id,
            persistence.runtime_epoch,
            current_playtime(persistence, world.resource::<RuntimeState>().sim_now),
            transform,
            world
                .entity(entity)
                .get::<PlayerMotion>()
                .map(|motion| motion.0),
            current_resources(stats),
        ),
        player_progression_snapshot(progression),
    );
    let _ = world.despawn(entity);
    Ok(snapshot)
}

fn player_snapshot(
    runtime: PlayerRuntimeSnapshot,
    progression: PlayerProgressionSnapshot,
) -> PlayerSnapshot {
    PlayerSnapshot {
        runtime,
        progression,
    }
}

fn player_progression_snapshot(progression: &PlayerProgressionComp) -> PlayerProgressionSnapshot {
    PlayerProgressionSnapshot {
        core_stat_allocations: progression.0.core_stat_allocations,
        stat_reset_count: progression.0.stat_reset_count,
    }
}

fn player_runtime_snapshot(
    map: &MapConfig,
    state: &RuntimeState,
    player_id: PlayerId,
    runtime_epoch: PlayerRuntimeEpoch,
    playtime: PlayerPlaytime,
    transform: &LocalTransform,
    motion: Option<super::PlayerMotionState>,
    resources: CurrentResources,
) -> PlayerRuntimeSnapshot {
    PlayerRuntimeSnapshot {
        id: player_id,
        runtime_epoch,
        map_key: map.map_code.clone(),
        playtime,
        current_hp: resources.hp,
        current_sp: resources.sp,
        current_stamina: resources.stamina,
        local_pos: snapshot_local_pos(map, state, transform, motion),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CurrentResources {
    hp: Option<i32>,
    sp: Option<i32>,
    stamina: Option<i32>,
}

fn current_resources(stats: &PlayerStatsComp) -> CurrentResources {
    let mut state = stats.state.clone();
    let api = GameStatsApi::new(&stats.source, &mut state);
    CurrentResources {
        hp: Some(api.read_packet(Stat::Hp)),
        sp: Some(api.read_packet(Stat::Sp)),
        stamina: Some(api.read_packet(Stat::Stamina)),
    }
}

fn advance_autosave_deadline(persistence: &mut PlayerPersistenceState, now: SimInstant) {
    while persistence.next_autosave_at <= now {
        persistence.next_autosave_at = persistence
            .next_autosave_at
            .saturating_add(AUTOSAVE_INTERVAL);
    }
}

fn current_playtime(persistence: &PlayerPersistenceState, now: SimInstant) -> PlayerPlaytime {
    let elapsed: Duration = now.saturating_sub(persistence.entered_map_at).into();
    persistence.persisted_playtime.saturating_add(elapsed)
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
        PlayerPersistencePort, PlayerPersistenceRequest, SaveUrgency, player_persistence_channel,
    };
    use crate::runtime::common::NetEntityId;
    use crate::runtime::player::{
        PendingDurableFlush, PlayerMotionState, PlayerPendingDurableFlush, PlayerProgressionComp,
        PlayerStatsComp,
    };
    use crate::runtime::time::SimInstant;
    use crate::types::MapInstanceKey;
    use crate::{MapConfig, SharedConfig, WanderConfig};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::oneshot;
    use zohar_domain::MapId;
    use zohar_domain::TerrainFlags;
    use zohar_domain::coords::{LocalPos, LocalSize};
    use zohar_domain::entity::EntityId;
    use zohar_domain::entity::player::{
        CoreStatAllocations, PlayerClass, PlayerGameplayBootstrap, PlayerProgressionSnapshot,
    };
    use zohar_gameplay::stats::game::{
        ActorStatSource, CoreStatBlock, DeterministicGrowthVersion, PlayerGrowthFormula,
        PlayerResourceFormula, PlayerStatSource, SourceSpeeds,
    };
    use zohar_map_port::ClientTimestamp;
    use zohar_map_port::Facing72;

    fn test_player_stat_rules() -> crate::PlayerStatRules {
        crate::PlayerStatRules::new(
            crate::PlayerClassStatsTable::new(vec![(
                PlayerClass::Warrior,
                crate::PlayerClassStatsConfig {
                    base_stats: CoreStatBlock::new(6, 4, 3, 3),
                    stat_source: ActorStatSource::Player(PlayerStatSource {
                        resources: PlayerResourceFormula {
                            base_max_hp: 600,
                            base_max_sp: 200,
                            base_max_stamina: 800,
                            hp_per_ht: 40,
                            sp_per_iq: 20,
                            stamina_per_ht: 5,
                        },
                        growth: PlayerGrowthFormula {
                            hp_per_level: (36, 44),
                            sp_per_level: (18, 22),
                            stamina_per_level: (5, 8),
                            version: DeterministicGrowthVersion::V1,
                        },
                        balance: zohar_gameplay::stats::game::default_player_balance_rules(
                            PlayerClass::Warrior,
                        ),
                        speeds: SourceSpeeds::default(),
                    }),
                },
            )]),
            crate::LevelExpTable::new((1..=120).map(|level| crate::LevelExpEntry {
                level,
                next_exp: i64::from(level) * 1_000,
                death_loss_pct: 5,
            })),
        )
    }

    fn test_shared_config() -> SharedConfig {
        SharedConfig {
            motion_speeds: Arc::new(EntityMotionSpeedTable::default()),
            mobs: Arc::new(HashMap::new()),
            player_stats: Arc::new(test_player_stat_rules()),
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

    fn test_progression() -> PlayerProgressionComp {
        PlayerProgressionComp(PlayerGameplayBootstrap {
            player_id: PlayerId::from(1),
            class: PlayerClass::Warrior,
            level: 1,
            exp_in_level: 0,
            core_stat_allocations: CoreStatAllocations::default(),
            stat_reset_count: 0,
            current_hp: None,
            current_sp: None,
            current_stamina: None,
        })
    }

    fn test_stats(current_hp: i32, current_sp: i32, current_stamina: i32) -> PlayerStatsComp {
        let mut gameplay = test_progression().0;
        gameplay.current_hp = Some(current_hp);
        gameplay.current_sp = Some(current_sp);
        gameplay.current_stamina = Some(current_stamina);
        let hydrated = test_player_stat_rules()
            .hydrate_player(&gameplay)
            .expect("player stats should hydrate for tests");
        PlayerStatsComp {
            source: hydrated.source,
            state: hydrated.state,
        }
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
                test_progression(),
                test_stats(321, 123, 777),
                PlayerPersistenceState {
                    dirty: true,
                    runtime_epoch: Default::default(),
                    persisted_playtime: PlayerPlaytime::from_secs(120),
                    entered_map_at: SimInstant::from_millis(5_000),
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();

        app.update();

        let request = rx.try_recv().expect("autosave request");
        match request {
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot,
                urgency,
                reply,
            } => {
                assert_eq!(snapshot.player_id(), player_id);
                assert!(matches!(urgency, SaveUrgency::Autosave));
                assert!(reply.is_none());
                assert_eq!(snapshot.runtime.map_key, "zohar_map_a1");
                assert_eq!(snapshot.runtime.playtime.as_secs(), 145);
                assert_eq!(snapshot.runtime.local_pos, LocalPos::new(11.0, 22.0));
                assert_eq!(snapshot.runtime.current_hp, Some(321));
                assert_eq!(snapshot.runtime.current_sp, Some(123));
                assert_eq!(snapshot.runtime.current_stamina, Some(777));
                assert_eq!(
                    snapshot.progression,
                    PlayerProgressionSnapshot {
                        core_stat_allocations: CoreStatAllocations::default(),
                        stat_reset_count: 0,
                    }
                );
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
            test_progression(),
            test_stats(450, 210, 699),
            PlayerMotion(PlayerMotionState {
                segment_start_pos: LocalPos::new(4.0, 4.0),
                segment_end_pos: LocalPos::new(8.0, 4.0),
                segment_start_ts: ClientTimestamp::new(1_000),
                segment_end_ts: ClientTimestamp::new(5_000),
                last_client_ts: ClientTimestamp::new(1_000),
            }),
            PlayerPersistenceState {
                dirty: true,
                runtime_epoch: Default::default(),
                persisted_playtime: PlayerPlaytime::ZERO,
                entered_map_at: SimInstant::ZERO,
                next_autosave_at: SimInstant::ZERO,
            },
        ));

        app.update();

        let request = rx.try_recv().expect("autosave request");
        match request {
            PlayerPersistenceRequest::SaveSnapshot { snapshot, .. } => {
                assert_eq!(snapshot.runtime.playtime.as_secs(), 3);
                assert_eq!(snapshot.runtime.local_pos, LocalPos::new(6.0, 4.0));
                assert_eq!(snapshot.runtime.current_hp, Some(450));
                assert_eq!(snapshot.runtime.current_sp, Some(210));
                assert_eq!(snapshot.runtime.current_stamina, Some(699));
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn enqueue_due_autosaves_skips_players_with_pending_flush() {
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
        let (_reply_tx, reply_rx) = oneshot::channel();
        let entity = app
            .world_mut()
            .spawn((
                PlayerMarker { player_id },
                LocalTransform {
                    pos: LocalPos::new(11.0, 22.0),
                    rot: Facing72::from_wrapped(0),
                },
                test_progression(),
                test_stats(321, 123, 777),
                PlayerPendingDurableFlush(Some(PendingDurableFlush { reply_rx })),
                PlayerPersistenceState {
                    dirty: true,
                    runtime_epoch: Default::default(),
                    persisted_playtime: PlayerPlaytime::from_secs(120),
                    entered_map_at: SimInstant::from_millis(5_000),
                    next_autosave_at: SimInstant::ZERO,
                },
            ))
            .id();

        app.update();

        assert!(
            rx.try_recv().is_err(),
            "pending flush should suppress autosave"
        );
        let persistence = app
            .world()
            .entity(entity)
            .get::<PlayerPersistenceState>()
            .expect("persistence state");
        assert!(persistence.dirty);
        assert_eq!(persistence.next_autosave_at, SimInstant::ZERO);
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
                test_progression(),
                test_stats(555, 222, 444),
                PlayerPersistenceState {
                    dirty: true,
                    runtime_epoch: Default::default(),
                    persisted_playtime: PlayerPlaytime::from_secs(90),
                    entered_map_at: SimInstant::ZERO,
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
        assert!(navigator.can_stand(snapshot.runtime.local_pos));
        assert_eq!(snapshot.runtime.playtime.as_secs(), 90);
        assert_eq!(snapshot.runtime.current_hp, Some(555));
        assert_eq!(snapshot.runtime.current_sp, Some(222));
        assert_eq!(snapshot.runtime.current_stamina, Some(444));
        assert!(snapshot.runtime.local_pos.x < 2.0);
        assert!(snapshot.runtime.local_pos.x > 1.9);
        assert_eq!(snapshot.runtime.local_pos.y, 1.2);
        assert_eq!(snapshot.progression.stat_reset_count, 0);
    }
}
