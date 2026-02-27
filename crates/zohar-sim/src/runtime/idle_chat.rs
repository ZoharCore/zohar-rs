use bevy::prelude::*;
use rand::{Rng, RngExt};

use crate::api::PlayerEvent;

use super::players::player_entities_on_map;
use super::state::{
    MobChatState, MobRef, NetEntityId, PlayerOutboxComp, RuntimeState, SharedConfig,
};

const CHAT_CONTEXT_IDLE: &str = "idle";

pub(super) fn emit_idle_chat(world: &mut World) {
    let recipients = player_entities_on_map(world);
    if recipients.is_empty() {
        return;
    }

    let shared = world.resource::<SharedConfig>().clone();
    let now_ms = world.resource::<RuntimeState>().sim_time_ms;

    let mob_entities: Vec<(Entity, zohar_domain::MobId, zohar_domain::entity::EntityId)> = {
        let mut query = world.query::<(Entity, &MobRef, &NetEntityId)>();
        query
            .iter(world)
            .map(|(entity, mob_ref, net_entity_id)| (entity, mob_ref.mob_id, net_entity_id.net_id))
            .collect()
    };

    let mut emissions = Vec::<(zohar_domain::entity::EntityId, Vec<u8>)>::new();

    for (mob_entity, mob_id, mob_net_id) in mob_entities {
        let Some(proto) = shared.mobs.get(&mob_id) else {
            continue;
        };

        let Some(strategy) =
            shared
                .mob_chat
                .strategy_for(CHAT_CONTEXT_IDLE, proto.mob_kind, mob_id)
        else {
            continue;
        };

        let Some(lines) = shared.mob_chat.lines_for(CHAT_CONTEXT_IDLE, mob_id) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }

        let should_emit = {
            let next_delay_ms = {
                let mut state = world.resource_mut::<RuntimeState>();
                let min_ms = u64::from(strategy.interval_min_sec).saturating_mul(1000);
                let max_ms = u64::from(strategy.interval_max_sec).saturating_mul(1000);
                random_interval_ms(&mut state.rng, min_ms, max_ms)
            };

            let mut ent = world.entity_mut(mob_entity);
            if let Some(mut chat_state) = ent.get_mut::<MobChatState>() {
                if now_ms < chat_state.next_emit_at_ms {
                    false
                } else {
                    chat_state.next_emit_at_ms = now_ms.saturating_add(next_delay_ms);
                    true
                }
            } else {
                ent.insert(MobChatState {
                    next_emit_at_ms: now_ms.saturating_add(next_delay_ms),
                });
                false
            }
        };

        if !should_emit {
            continue;
        }

        let line_idx = {
            let mut state = world.resource_mut::<RuntimeState>();
            state.rng.random_range(0..lines.len())
        };
        let mut message = lines[line_idx].text.as_bytes().to_vec();
        if !message.ends_with(&[0]) {
            message.push(0);
        }
        emissions.push((mob_net_id, message));
    }

    if emissions.is_empty() {
        return;
    }

    for recipient in recipients {
        if let Some(mut outbox) = world.entity_mut(recipient).get_mut::<PlayerOutboxComp>() {
            for (sender_entity_id, message) in &emissions {
                outbox.0.push_reliable(PlayerEvent::Chat {
                    kind: 0,
                    sender_entity_id: Some(*sender_entity_id),
                    empire: None,
                    message: message.clone(),
                });
            }
        }
    }
}

fn random_interval_ms(rng: &mut impl Rng, min_ms: u64, max_ms: u64) -> u64 {
    if min_ms >= max_ms {
        min_ms
    } else {
        rng.random_range(min_ms..=max_ms)
    }
}
