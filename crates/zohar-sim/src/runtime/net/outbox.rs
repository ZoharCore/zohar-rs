use bevy::prelude::*;

use super::players::player_entities_on_map;
use super::state::PlayerOutboxComp;

pub(crate) fn outbox_flush(world: &mut World) {
    let player_entities = player_entities_on_map(world);

    for entity in player_entities {
        if let Some(mut outbox) = world.entity_mut(entity).get_mut::<PlayerOutboxComp>() {
            let _ = outbox.0.flush();
        }
    }
}
