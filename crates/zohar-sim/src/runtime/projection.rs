use bevy::prelude::*;
use zohar_domain::entity::EntityId;
use zohar_gameplay::stats::game::PlayerStaminaTimerCommand;
use zohar_map_port::{ChatChannel, PlayerEvent};

use super::facts::{FrameFacts, PlayerStaminaTimerChanged, reset_frame_facts};
use super::state::{
    MapReplication, MobStatsComp, NetEntityIndex, PlayerMarker, PlayerOutboxComp, RuntimeState,
};

/// Project accumulated frame facts into map-port events.
///
/// This is the only runtime layer that should translate combat/lifecycle facts into client-facing
/// packets. Keeping that boundary narrow lets later reward, PvP, buff, or quest hooks observe
/// the same facts without being coupled to legacy wire concerns.
pub(crate) fn project_frame_facts(world: &mut World) {
    let damaged = world.resource::<FrameFacts>().combat.damaged.clone();
    for effect in damaged {
        if world
            .entity(effect.victim.entity)
            .contains::<MobStatsComp>()
        {
            crate::runtime::player::target::broadcast_entity_health_bar_to_targeters(
                world,
                effect.victim.id,
            );
        }
        if world
            .entity(effect.attacker.entity)
            .contains::<PlayerMarker>()
        {
            crate::runtime::player::target::send_damage_info_to_selected_target(
                world,
                effect.attacker.entity,
                effect.victim.id,
                effect.damage,
                effect.flags,
            );
        }
        if world
            .entity(effect.victim.entity)
            .contains::<PlayerMarker>()
        {
            crate::runtime::player::target::send_damage_info_to_player(
                world,
                effect.victim.entity,
                effect.victim.id,
                effect.damage,
                effect.flags,
            );
        }
    }

    let dying_started = world.resource::<FrameFacts>().life.dying_started.clone();
    for effect in dying_started {
        broadcast_lifecycle_event(world, effect.actor.id, |entity_id| {
            PlayerEvent::EntityStunned { entity_id }
        });
    }

    let death_finalized = world.resource::<FrameFacts>().life.death_finalized.clone();
    for effect in death_finalized {
        broadcast_lifecycle_event(world, effect.actor.id, |entity_id| {
            PlayerEvent::EntityDead { entity_id }
        });
    }

    let despawned = world.resource::<FrameFacts>().cleanup.despawned.clone();
    for effect in despawned {
        for player_entity in effect.recipients {
            push_reliable(
                world,
                player_entity,
                PlayerEvent::EntityDespawn {
                    entity_id: effect.actor_id,
                },
            );
        }
    }

    let stamina_timers = world
        .resource::<FrameFacts>()
        .projection
        .stamina_timer_changed
        .clone();
    for fact in stamina_timers {
        push_reliable(
            world,
            fact.player.entity,
            PlayerEvent::Chat {
                channel: ChatChannel::Command,
                message: stamina_timer_command(fact).into_bytes(),
                sender_entity_id: None,
                empire: None,
            },
        );
    }

    reset_frame_facts(world);
}

fn broadcast_lifecycle_event(
    world: &mut World,
    target_id: EntityId,
    make_event: impl Fn(EntityId) -> PlayerEvent,
) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let Some(target_entity) = world
        .resource::<NetEntityIndex>()
        .0
        .get(&target_id)
        .copied()
    else {
        return;
    };

    let mut recipient_ids = world
        .entity(map_entity)
        .get::<MapReplication>()
        .map(|replication| replication.0.observers_for(target_id))
        .unwrap_or_default();
    if world.entity(target_entity).contains::<PlayerMarker>() {
        recipient_ids.push(target_id);
    }

    recipient_ids.sort_unstable_by_key(|entity_id| entity_id.0);
    recipient_ids.dedup();

    let recipients = recipient_ids
        .into_iter()
        .filter_map(|recipient_id| {
            world
                .resource::<NetEntityIndex>()
                .0
                .get(&recipient_id)
                .copied()
        })
        .collect::<Vec<_>>();

    for player_entity in recipients {
        push_reliable(world, player_entity, make_event(target_id));
    }
}

fn push_reliable(world: &mut World, player_entity: Entity, event: PlayerEvent) {
    if let Some(mut outbox) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerOutboxComp>()
    {
        outbox.0.push_reliable(event);
    }
}

fn stamina_timer_command(fact: PlayerStaminaTimerChanged) -> String {
    match fact.command {
        PlayerStaminaTimerCommand::Start { consume_per_sec } => {
            format!(
                "StartStaminaConsume {consume_per_sec} {}\0",
                fact.current_stamina
            )
        }
        PlayerStaminaTimerCommand::Stop => {
            format!("StopStaminaConsume {}\0", fact.current_stamina)
        }
    }
}
