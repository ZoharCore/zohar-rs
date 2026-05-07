use bevy::prelude::*;
use zohar_domain::entity::EntityId;
use zohar_map_port::PlayerEvent;

use super::state::{MapReplication, NetEntityIndex, PlayerOutboxComp, RuntimeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorAudience {
    /// Players who currently observe this actor, excluding the actor's own player outbox.
    Observers,
    /// Observers plus the actor itself when the actor is a player.
    ViewAndSelf,
}

pub(crate) fn broadcast_actor_event(
    world: &mut World,
    actor_id: EntityId,
    audience: ActorAudience,
    make_event: impl Fn(EntityId) -> PlayerEvent,
) {
    let recipients = actor_audience_recipients(world, actor_id, audience);
    for player_entity in recipients {
        let event = make_event(actor_id);
        push_reliable(world, player_entity, event);
    }
}

pub(crate) fn push_reliable(world: &mut World, player_entity: Entity, event: PlayerEvent) {
    if let Some(mut outbox) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerOutboxComp>()
    {
        outbox.0.push_reliable(event);
    }
}

fn actor_audience_recipients(
    world: &World,
    actor_id: EntityId,
    audience: ActorAudience,
) -> Vec<Entity> {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return Vec::new();
    };
    let mut recipient_ids = world
        .entity(map_entity)
        .get::<MapReplication>()
        .map(|replication| replication.0.observers_for(actor_id))
        .unwrap_or_default();
    if audience == ActorAudience::ViewAndSelf {
        recipient_ids.push(actor_id);
    }
    recipient_ids.sort_unstable_by_key(|entity_id| entity_id.0);
    recipient_ids.dedup();

    recipient_ids
        .into_iter()
        .filter_map(|recipient_id| {
            world
                .resource::<NetEntityIndex>()
                .0
                .get(&recipient_id)
                .copied()
        })
        .collect()
}
