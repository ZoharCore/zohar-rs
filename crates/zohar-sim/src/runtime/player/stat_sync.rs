use bevy::prelude::*;
use zohar_gameplay::stats::game::StatDelta;
use zohar_map_port::{PlayerEvent, StatUpdate};

use super::persistence::mark_player_dirty;
use super::state::{NetEntityId, PlayerAppearanceComp, PlayerOutboxComp, PlayerStatsComp};

pub(crate) fn normalize_player_stats_for_gameplay(
    mut query: Query<(&mut PlayerStatsComp, &mut PlayerAppearanceComp)>,
) {
    for (mut stats, mut appearance) in &mut query {
        if let Some(update) = stats.0.normalize().public_state {
            apply_public_state(&mut appearance, update);
        }
    }
}

pub(crate) fn flush_player_stats_sync(world: &mut World) {
    let player_entities = super::players::player_entities_on_map(world);

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        let (entity_id, drained) = {
            let mut query = world.query::<(
                &NetEntityId,
                &mut PlayerStatsComp,
                &mut PlayerAppearanceComp,
            )>();
            let Ok((net_id, mut stats, mut appearance)) = query.get_mut(world, player_entity)
            else {
                continue;
            };

            let drained = stats.0.drain_sync();
            if let Some(update) = drained.public_state {
                apply_public_state(&mut appearance, update);
            }
            (net_id.net_id, drained)
        };

        if drained.is_empty() {
            continue;
        }

        if drained.stat_deltas.is_empty() {
            continue;
        }

        mark_player_dirty(world, player_entity);

        let mut query = world.query::<&mut PlayerOutboxComp>();
        let Ok(mut outbox) = query.get_mut(world, player_entity) else {
            continue;
        };
        outbox.0.push_reliable(PlayerEvent::SetEntityStats {
            entity_id,
            stats: stat_updates_from_deltas(drained.stat_deltas),
        });
    }
}

pub(crate) fn stat_updates_from_deltas(
    stat_deltas: impl IntoIterator<Item = StatDelta>,
) -> Vec<StatUpdate> {
    stat_deltas
        .into_iter()
        .map(|delta| StatUpdate {
            stat: delta.stat,
            absolute: delta.value,
        })
        .collect()
}

fn apply_public_state(
    appearance: &mut PlayerAppearanceComp,
    update: zohar_gameplay::stats::game::ActorPublicState,
) {
    appearance.0.level = update.stats.level;
    appearance.0.move_speed = update.stats.move_speed;
    appearance.0.attack_speed = update.stats.attack_speed;
}
