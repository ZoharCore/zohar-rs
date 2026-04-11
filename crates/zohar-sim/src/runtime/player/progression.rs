use bevy::prelude::*;
use tokio::sync::oneshot::error::TryRecvError;
use zohar_domain::entity::player::{CoreStatKind, PlayerId, PlayerProgressionSnapshot};
use zohar_gameplay::stats::game::{GameStatsApi, Stat, StatWriteError};
use zohar_map_port::{ChatChannel, PlayerEvent, PlayerProgressionIntent};

use crate::persistence::PlayerPersistencePort;

use super::PendingDurableFlush;
use super::persistence::{capture_player_snapshot, mark_player_dirty};
use super::state::{
    NetEntityId, PlayerAppearanceComp, PlayerMarker, PlayerOutboxComp, PlayerPendingDurableFlush,
    PlayerProgressionComp, PlayerProgressionIntentQueue, PlayerStatsComp, SharedConfig,
};

pub(crate) fn process_player_progression(world: &mut World) {
    let player_entities = super::players::player_entities_on_map(world);

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        finalize_pending_save(world, player_entity);
        if has_pending_save(world, player_entity) {
            continue;
        }

        enqueue_progression_intents(world, player_entity);
        finalize_pending_save(world, player_entity);
    }
}

fn finalize_pending_save(world: &mut World, player_entity: Entity) {
    let poll = {
        let mut query = world.query::<&mut PlayerPendingDurableFlush>();
        let Ok(mut pending) = query.get_mut(world, player_entity) else {
            return;
        };
        let Some(save) = pending.0.as_mut() else {
            return;
        };

        match save.reply_rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Closed) => Some(Err("player state flush reply dropped".to_string())),
        }
    };

    let Some(result) = poll else {
        return;
    };

    let had_pending = {
        let mut query = world.query::<&mut PlayerPendingDurableFlush>();
        let Ok(mut pending) = query.get_mut(world, player_entity) else {
            return;
        };
        pending.0.take().is_some()
    };
    if !had_pending {
        return;
    }

    match result {
        Ok(()) => {}
        Err(error) => {
            mark_player_dirty(world, player_entity);
            push_feedback(
                world,
                player_entity,
                format!("{error}; player state will retry on the next autosave."),
            );
        }
    }
}

fn has_pending_save(world: &mut World, player_entity: Entity) -> bool {
    let mut query = world.query::<&PlayerPendingDurableFlush>();
    query
        .get(world, player_entity)
        .map(|pending| pending.0.is_some())
        .unwrap_or(false)
}

fn enqueue_progression_intents(world: &mut World, player_entity: Entity) {
    let intents = {
        let mut query = world.query::<&mut PlayerProgressionIntentQueue>();
        let Ok(mut queue) = query.get_mut(world, player_entity) else {
            return;
        };
        std::mem::take(&mut queue.0)
    };

    if intents.is_empty() {
        return;
    }

    let mut remaining = Vec::new();
    let mut blocked = false;

    for intent in intents {
        if blocked {
            remaining.push(intent);
            continue;
        }

        match enqueue_progression_intent(world, player_entity, intent) {
            Ok(Some(save)) => {
                let mut query = world.query::<&mut PlayerPendingDurableFlush>();
                if let Ok(mut pending) = query.get_mut(world, player_entity) {
                    pending.0 = Some(save);
                }
                blocked = true;
            }
            Ok(None) => {}
            Err(error) => push_feedback(world, player_entity, error.to_string()),
        }
    }

    if !remaining.is_empty() {
        let mut query = world.query::<&mut PlayerProgressionIntentQueue>();
        if let Ok(mut queue) = query.get_mut(world, player_entity) {
            queue.0.splice(0..0, remaining);
        }
    }
}

fn enqueue_progression_intent(
    world: &mut World,
    player_entity: Entity,
    intent: PlayerProgressionIntent,
) -> Result<Option<PendingDurableFlush>, ProgressionError> {
    match intent {
        PlayerProgressionIntent::CoreStat(intent) => {
            let validated =
                validate_core_stat_intent(world, player_entity, intent.stat, intent.delta)?;
            let progression = validated.progression.clone();
            let snapshot = capture_player_snapshot(world, player_entity, progression)
                .map_err(|_| ProgressionError::MissingPlayerState)?;
            let reply_rx = world
                .resource::<PlayerPersistencePort>()
                .handle()
                .try_schedule_flush(snapshot)
                .map_err(|source| ProgressionError::PersistenceQueue { source })?;
            apply_validated_core_stat_save(world, player_entity, validated)?;

            Ok(Some(PendingDurableFlush { reply_rx }))
        }
        PlayerProgressionIntent::SkillLevel(_intent) => Err(ProgressionError::SkillNotImplemented),
    }
}

fn validate_core_stat_intent(
    world: &mut World,
    player_entity: Entity,
    stat: CoreStatKind,
    delta: i8,
) -> Result<ValidatedCoreStatSave, ProgressionError> {
    let shared = world.resource::<SharedConfig>().clone();
    let mut query = world.query::<(&PlayerMarker, &PlayerProgressionComp, &PlayerStatsComp)>();
    let (marker, progression, stats) = query
        .get(world, player_entity)
        .map_err(|_| ProgressionError::MissingPlayerState)?;

    let current_stats = shared
        .player_stats
        .resolve_player_stats(progression.0.class, progression.0.core_stat_allocations)
        .ok_or(ProgressionError::MissingPlayerClassConfig {
            player_id: marker.player_id,
            stat,
        })?;

    let mut allocations = progression.0.core_stat_allocations;
    let mut stat_reset_count = progression.0.stat_reset_count;
    let (current_absolute, allocation_slot) = match stat {
        CoreStatKind::St => (current_stats.stat_str, &mut allocations.allocated_str),
        CoreStatKind::Ht => (current_stats.stat_vit, &mut allocations.allocated_vit),
        CoreStatKind::Dx => (current_stats.stat_dex, &mut allocations.allocated_dex),
        CoreStatKind::Iq => (current_stats.stat_int, &mut allocations.allocated_int),
    };

    let (new_absolute, stat_points_delta) = match delta {
        1 => {
            if stat_reset_count < 0 {
                return Err(ProgressionError::InvalidProgressionState);
            }
            if current_stat_points(stats) <= 0 {
                return Err(ProgressionError::NoStatPoints);
            }
            *allocation_slot += 1;
            (current_absolute + 1, -1)
        }
        -1 => {
            if stat_reset_count <= 0 {
                return Err(ProgressionError::NoStatResetPoints);
            }
            if *allocation_slot <= 0 {
                return Err(ProgressionError::StatAtBaseFloor { stat });
            }
            *allocation_slot -= 1;
            stat_reset_count -= 1;
            (current_absolute - 1, 1)
        }
        other => return Err(ProgressionError::UnsupportedDelta(other)),
    };

    let mut state = stats.state.clone();
    let mut api = GameStatsApi::new(&stats.source, &mut state);
    api.set_stored_stat(core_stat_to_stat(stat), new_absolute)
        .map_err(|error| map_stat_write_error(core_stat_to_stat(stat), error))?;

    Ok(ValidatedCoreStatSave {
        stat,
        progression: PlayerProgressionSnapshot {
            core_stat_allocations: allocations,
            stat_reset_count,
        },
        new_absolute,
        stat_points_delta,
    })
}

fn apply_validated_core_stat_save(
    world: &mut World,
    player_entity: Entity,
    validated: ValidatedCoreStatSave,
) -> Result<(), ProgressionError> {
    let mut query = world.query::<(
        &NetEntityId,
        &mut PlayerProgressionComp,
        &mut PlayerStatsComp,
        &mut PlayerAppearanceComp,
        &mut PlayerOutboxComp,
    )>();
    let (net_id, mut progression, mut stats, mut appearance, mut outbox) = query
        .get_mut(world, player_entity)
        .map_err(|_| ProgressionError::MissingPlayerState)?;

    apply_stat_kernel_update(
        &mut stats,
        &mut appearance,
        &mut outbox,
        net_id.net_id,
        validated.stat,
        validated.new_absolute,
        validated.progression.stat_reset_count,
        validated.stat_points_delta,
    )?;

    progression.0.core_stat_allocations = validated.progression.core_stat_allocations;
    progression.0.stat_reset_count = validated.progression.stat_reset_count;
    Ok(())
}

fn apply_stat_kernel_update(
    stats: &mut PlayerStatsComp,
    appearance: &mut PlayerAppearanceComp,
    outbox: &mut PlayerOutboxComp,
    entity_id: zohar_domain::entity::EntityId,
    stat: CoreStatKind,
    new_absolute: i32,
    stat_reset_count: i32,
    stat_points_delta: i32,
) -> Result<(), ProgressionError> {
    let stat_id = core_stat_to_stat(stat);
    let mut api = GameStatsApi::new(&stats.source, &mut stats.state);
    let before = api.stat_snapshot();
    let current_stat_points = api.read_packet(Stat::StatPoints);

    api.set_stored_stat(stat_id, new_absolute)
        .map_err(|error| map_stat_write_error(stat_id, error))?;
    api.set_stored_stat(Stat::StatPoints, current_stat_points + stat_points_delta)
        .map_err(|error| map_stat_write_error(Stat::StatPoints, error))?;
    api.set_stored_stat(Stat::StatResetCount, stat_reset_count)
        .map_err(|error| map_stat_write_error(Stat::StatResetCount, error))?;

    let sync = api.sync_if_dirty();
    if let Some(update) = sync.character_update {
        appearance.0.level = update.appearance.level;
        appearance.0.move_speed = update.appearance.move_speed;
        appearance.0.attack_speed = update.appearance.attack_speed;
    }

    for stat_delta in sync.stat_deltas {
        let previous = before.get(stat_delta.stat) as i32;
        outbox.0.push_reliable(PlayerEvent::SetEntityStat {
            entity_id,
            stat: stat_delta.stat,
            delta: stat_delta.value - previous,
            absolute: stat_delta.value,
        });
    }

    Ok(())
}

fn push_feedback(world: &mut World, player_entity: Entity, message: impl Into<String>) {
    let mut entity = world.entity_mut(player_entity);
    let Some(mut outbox) = entity.get_mut::<PlayerOutboxComp>() else {
        return;
    };
    outbox.0.push_reliable(PlayerEvent::Chat {
        channel: ChatChannel::Info,
        sender_entity_id: None,
        empire: None,
        message: format!("{}\0", message.into()).into_bytes(),
    });
}

fn current_stat_points(stats: &PlayerStatsComp) -> i32 {
    let mut state = stats.state.clone();
    let api = GameStatsApi::new(&stats.source, &mut state);
    api.read_packet(Stat::StatPoints)
}

fn core_stat_to_stat(stat: CoreStatKind) -> Stat {
    match stat {
        CoreStatKind::St => Stat::St,
        CoreStatKind::Ht => Stat::Ht,
        CoreStatKind::Dx => Stat::Dx,
        CoreStatKind::Iq => Stat::Iq,
    }
}

fn map_stat_write_error(stat: Stat, error: StatWriteError) -> ProgressionError {
    match error {
        StatWriteError::OutOfRange {
            min, max, value, ..
        } => {
            if matches!(stat, Stat::St | Stat::Ht | Stat::Dx | Stat::Iq) && value > max {
                ProgressionError::StatAtCap {
                    stat: core_stat_from_stat(stat).expect("core stat"),
                    cap: max,
                }
            } else {
                ProgressionError::StatWriteOutOfRange {
                    stat,
                    min,
                    max,
                    attempted: value,
                }
            }
        }
        other => ProgressionError::StatWriteFailed { stat, error: other },
    }
}

fn core_stat_from_stat(stat: Stat) -> Option<CoreStatKind> {
    match stat {
        Stat::St => Some(CoreStatKind::St),
        Stat::Ht => Some(CoreStatKind::Ht),
        Stat::Dx => Some(CoreStatKind::Dx),
        Stat::Iq => Some(CoreStatKind::Iq),
        _ => None,
    }
}

struct ValidatedCoreStatSave {
    stat: CoreStatKind,
    progression: PlayerProgressionSnapshot,
    new_absolute: i32,
    stat_points_delta: i32,
}

#[derive(Debug, thiserror::Error)]
enum ProgressionError {
    #[error("Player state is incomplete.")]
    MissingPlayerState,

    #[error("Missing player stat rules for {player_id:?} {stat:?}.")]
    MissingPlayerClassConfig {
        player_id: PlayerId,
        stat: CoreStatKind,
    },

    #[error("Player progression state is invalid.")]
    InvalidProgressionState,

    #[error("No stat points available.")]
    NoStatPoints,

    #[error("No stat reset points available.")]
    NoStatResetPoints,

    #[error("{stat:?} is already at its class base value.")]
    StatAtBaseFloor { stat: CoreStatKind },

    #[error("{stat:?} is already at max ({cap}).")]
    StatAtCap { stat: CoreStatKind, cap: i32 },

    #[error("Skill progression is not implemented yet.")]
    SkillNotImplemented,

    #[error("Unsupported progression delta {0}.")]
    UnsupportedDelta(i8),

    #[error("Progression persistence queue failure: {source}.")]
    PersistenceQueue {
        #[from]
        source: crate::persistence::PlayerPersistenceQueueError,
    },

    #[error("{stat:?} write out of range ({attempted}, expected {min:?}..={max}).")]
    StatWriteOutOfRange {
        stat: Stat,
        min: Option<i32>,
        max: i32,
        attempted: i32,
    },

    #[error("Failed to write {stat:?}: {error}.")]
    StatWriteFailed { stat: Stat, error: StatWriteError },
}
