use super::players::player_entities_on_map;
use super::state::{
    ChatIntentQueue, MapPendingLocalChats, NetEntityId, PendingLocalChat, PlayerAppearanceComp,
    PlayerMarker, RuntimeState,
};
use bevy::prelude::*;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::PlayerId;

pub(crate) fn process_chat_intents(world: &mut World) {
    let player_entities = player_entities_on_map(world);

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        let (player_id, appearance_empire, player_name, mover_net_id) = {
            let e = world.entity(player_entity);
            let Some(player) = e.get::<PlayerMarker>() else {
                continue;
            };
            let Some(appearance) = e.get::<PlayerAppearanceComp>() else {
                continue;
            };
            let Some(net_id) = e.get::<NetEntityId>() else {
                continue;
            };
            (
                player.player_id,
                appearance.0.empire,
                appearance.0.name.clone(),
                net_id.net_id,
            )
        };

        let chat_intents = {
            let mut ent = world.entity_mut(player_entity);
            let Some(mut chat_queue) = ent.get_mut::<ChatIntentQueue>() else {
                continue;
            };
            std::mem::take(&mut chat_queue.0)
        };

        enqueue_local_chat_intents(
            world,
            player_id,
            mover_net_id,
            appearance_empire,
            &player_name,
            chat_intents,
        );
    }
}

fn enqueue_local_chat_intents(
    world: &mut World,
    speaker_player_id: PlayerId,
    speaker_entity_id: EntityId,
    speaker_empire: zohar_domain::Empire,
    speaker_name: &str,
    chat_intents: Vec<super::state::ChatIntent>,
) {
    if chat_intents.is_empty() {
        return;
    }

    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let mut map_ent = world.entity_mut(map_entity);
    let Some(mut pending_chats) = map_ent.get_mut::<MapPendingLocalChats>() else {
        return;
    };
    let speaker_name = speaker_name.to_string();

    for chat in chat_intents {
        pending_chats.0.push(PendingLocalChat {
            speaker_player_id,
            speaker_entity_id,
            speaker_empire,
            // TODO: only broadcast local speaking packets
            channel: chat.channel,
            speaker_name: speaker_name.clone(),
            message: chat.message,
        });
    }
}
