use bevy::prelude::*;
use zohar_domain::entity::EntityId;

use super::facts::{ActorDespawned, ActorRef, FrameFacts};
use super::state::{
    MapDirtyEntityPublicStates, MapPendingMovements, MapReplication, MapSpatial, MapSpawnRules,
    NetEntityIndex, RuntimeState, SimDuration, SimInstant,
};

pub(crate) fn process_cleanup_events(world: &mut World) {
    let actors = {
        let effects = world.resource::<FrameFacts>();
        effects
            .life
            .cleanup_due
            .iter()
            .map(|effect| effect.actor)
            .collect::<Vec<_>>()
    };

    for actor in actors {
        destroy_mob_entity(world, actor);
    }
}

fn destroy_mob_entity(world: &mut World, actor: ActorRef) {
    if !world.entities().contains(actor.entity) {
        return;
    }
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let observers = {
        let mut map_ent = world.entity_mut(map_entity);
        let Some(mut replication) = map_ent.get_mut::<MapReplication>() else {
            return;
        };
        replication.0.remove_target(actor.id)
    };

    let now = world.resource::<RuntimeState>().sim_now;
    {
        let mut map_ent = world.entity_mut(map_entity);
        if let Some(mut spatial) = map_ent.get_mut::<MapSpatial>() {
            spatial.0.remove(actor.id);
        }
        if let Some(mut pending) = map_ent.get_mut::<MapPendingMovements>() {
            pending.0.retain(|movement| movement.entity_id != actor.id);
        }
        if let Some(mut pending_public_states) = map_ent.get_mut::<MapDirtyEntityPublicStates>() {
            pending_public_states
                .0
                .retain(|pending_entity_id| *pending_entity_id != actor.id);
        }
        if let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() {
            release_mob_spawn_slot(&mut spawn_rules, actor.id, now);
        }
    }

    let recipients = observers
        .into_iter()
        .filter_map(|observer_id| {
            world
                .resource::<NetEntityIndex>()
                .0
                .get(&observer_id)
                .copied()
        })
        .collect::<Vec<_>>();
    world
        .resource_mut::<FrameFacts>()
        .cleanup
        .despawned
        .push(ActorDespawned {
            actor_id: actor.id,
            recipients,
        });

    world.resource_mut::<NetEntityIndex>().0.remove(&actor.id);
    let _ = world.despawn(actor.entity);
    world.resource_mut::<RuntimeState>().is_dirty = true;
}

fn release_mob_spawn_slot(spawn_rules: &mut MapSpawnRules, entity_id: EntityId, now: SimInstant) {
    for (idx, rule_state) in spawn_rules.rules.iter_mut().enumerate() {
        if !rule_state.entities.remove(&entity_id) {
            continue;
        }
        if rule_state.active_instances > 0 {
            rule_state.active_instances -= 1;
        }
        let respawn_at = now.saturating_add(SimDuration::from_millis(
            rule_state.rule.regen_time.as_millis().min(u64::MAX as u128) as u64,
        ));
        rule_state.respawn_at = Some(respawn_at);
        spawn_rules
            .scheduled_spawns
            .push(std::cmp::Reverse((respawn_at, idx)));
        break;
    }
}
