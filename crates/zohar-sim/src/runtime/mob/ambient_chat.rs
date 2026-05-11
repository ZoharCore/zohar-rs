use bevy::prelude::*;
use rand::{Rng, RngExt};
use zohar_domain::Empire;
use zohar_map_port::ChatChannel;

use super::state::{
    MapPendingLocalChats, MobChatState, MobRef, NetEntityId, PendingLocalChat, RuntimeState,
    SharedConfig,
};

const CHAT_CONTEXT_IDLE: &str = "idle";

pub(crate) fn emit_idle_chat(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let shared = world.resource::<SharedConfig>().clone();
    let now = world.resource::<RuntimeState>().sim_now;

    let mob_entities: Vec<(
        Entity,
        zohar_domain::entity::mob::MobId,
        zohar_domain::entity::EntityId,
    )> = {
        let mut query = world.query::<(Entity, &MobRef, &NetEntityId)>();
        query
            .iter(world)
            .map(|(entity, mob_ref, net_entity_id)| (entity, mob_ref.mob_id, net_entity_id.net_id))
            .collect()
    };

    let mut emissions = Vec::<(zohar_domain::entity::EntityId, Option<Empire>, Vec<u8>)>::new();

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
                if now < chat_state.next_emit_at {
                    false
                } else {
                    chat_state.next_emit_at = now.saturating_add(next_delay_ms);
                    true
                }
            } else {
                ent.insert(MobChatState {
                    next_emit_at: now.saturating_add(next_delay_ms),
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
        emissions.push((mob_net_id, proto.empire, message));
    }

    if emissions.is_empty() {
        return;
    }

    let mut map_ent = world.entity_mut(map_entity);
    let Some(mut pending_chats) = map_ent.get_mut::<MapPendingLocalChats>() else {
        return;
    };

    for (sender_entity_id, empire, message) in emissions {
        pending_chats.0.push(PendingLocalChat {
            speaker_player_id: None,
            speaker_entity_id: sender_entity_id,
            speaker_empire: empire,
            channel: ChatChannel::Speak,
            speaker_name: None,
            message,
        });
    }
}

fn random_interval_ms(rng: &mut impl Rng, min_ms: u64, max_ms: u64) -> super::state::SimDuration {
    if min_ms >= max_ms {
        super::state::SimDuration::from_millis(min_ms)
    } else {
        super::state::SimDuration::from_millis(rng.random_range(min_ms..=max_ms))
    }
}
