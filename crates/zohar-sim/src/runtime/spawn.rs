use bevy::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use zohar_domain::mob::{FacingStrategy, SpawnRule};
use zohar_domain::{MobId, MobKind};

use super::state::{
    MapEmpire, MapMarker, MapPendingLocalChats, MapPendingMovements, MapReplication, MapSpatial,
    MapSpawnRules, MobMarker, MobRef, NetEntityId, NetEntityIndex, RuntimeState, SharedConfig,
    SpawnRuleState, StartupReadySignal, WanderState,
};
use super::util::{
    degrees_to_protocol_rot, expand_spawn_template, next_entity_id, random_idle_decision_delay,
    random_protocol_rot,
};

pub(super) fn bootstrap_map_runtime(world: &mut World) {
    let map_config = world.resource::<super::state::MapConfig>();
    let sim_time_ms = world.resource::<RuntimeState>().sim_time_ms;

    let mut rules = Vec::new();
    let mut scheduled_spawns = BinaryHeap::new();
    for (idx, rule) in map_config.spawn_rules.iter().cloned().enumerate() {
        rules.push(SpawnRuleState {
            rule,
            entities: HashSet::new(),
            respawn_at_ms: Some(sim_time_ms),
        });
        scheduled_spawns.push(Reverse((sim_time_ms, idx)));
    }

    let map_entity = world
        .spawn((
            MapMarker,
            map_config.map_key,
            MapEmpire(map_config.empire),
            MapSpatial(crate::aoi::SpatialIndex::new()),
            MapReplication::default(),
            MapSpawnRules {
                rules,
                scheduled_spawns,
            },
            MapPendingLocalChats::default(),
            MapPendingMovements::default(),
        ))
        .id();

    world.resource_mut::<RuntimeState>().map_entity = Some(map_entity);
    preload_map_to_spawn_cap_world(world);
}

pub(super) fn signal_startup_ready(ready_signal: Option<Res<StartupReadySignal>>) {
    let Some(ready_signal) = ready_signal else {
        return;
    };
    let Ok(mut slot) = ready_signal.0.lock() else {
        return;
    };
    if let Some(tx) = slot.take() {
        let _ = tx.send(());
    }
}

pub(super) fn spawn_rules(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    loop {
        let sim_time_ms = world.resource::<RuntimeState>().sim_time_ms;
        let next = {
            let Some(spawn_rules) = world.entity(map_entity).get::<MapSpawnRules>() else {
                break;
            };
            spawn_rules.scheduled_spawns.peek().copied().map(|v| v.0)
        };

        let Some((respawn_at_ms, rule_index)) = next else {
            break;
        };
        if respawn_at_ms > sim_time_ms {
            break;
        }

        {
            let mut map_ent = world.entity_mut(map_entity);
            let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() else {
                break;
            };
            let _ = spawn_rules.scheduled_spawns.pop();
        }

        let should_spawn = {
            let mut map_ent = world.entity_mut(map_entity);
            match map_ent.get_mut::<MapSpawnRules>() {
                Some(mut spawn_rules) => {
                    if rule_index >= spawn_rules.rules.len() {
                        false
                    } else {
                        let rule_state = &mut spawn_rules.rules[rule_index];
                        if rule_state.respawn_at_ms != Some(respawn_at_ms) {
                            false
                        } else if rule_state.entities.len() >= rule_state.rule.max_count {
                            rule_state.respawn_at_ms = None;
                            false
                        } else {
                            true
                        }
                    }
                }
                None => false,
            }
        };

        if !should_spawn {
            continue;
        }

        let (rule, existing_count) = {
            let Some(spawn_rules) = world.entity(map_entity).get::<MapSpawnRules>() else {
                continue;
            };
            (
                spawn_rules.rules[rule_index].rule.clone(),
                spawn_rules.rules[rule_index].entities.len(),
            )
        };

        let remaining_for_rule = rule.max_count.saturating_sub(existing_count);
        let mut remaining_for_rule_tick = remaining_for_rule;

        while remaining_for_rule_tick > 0 {
            let mob_ids = {
                let mut state = world.resource_mut::<RuntimeState>();
                expand_spawn_template(&rule.template, &mut state.rng)
            };
            if mob_ids.is_empty() {
                break;
            }
            let before_group_budget = remaining_for_rule_tick;
            for mob_id in mob_ids {
                if remaining_for_rule_tick == 0 {
                    break;
                }

                let Some(entity_id) = spawn_one_mob(world, map_entity, &shared, &rule, mob_id)
                else {
                    continue;
                };

                let mut map_ent = world.entity_mut(map_entity);
                let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() else {
                    continue;
                };
                spawn_rules.rules[rule_index].entities.insert(entity_id);
                remaining_for_rule_tick = remaining_for_rule_tick.saturating_sub(1);
            }

            if remaining_for_rule_tick == before_group_budget {
                break;
            }
        }

        {
            let mut map_ent = world.entity_mut(map_entity);
            let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() else {
                continue;
            };
            let current = spawn_rules.rules[rule_index].entities.len();
            let target = spawn_rules.rules[rule_index].rule.max_count;
            if current < target {
                let retry_at = sim_time_ms.saturating_add(1);
                spawn_rules.rules[rule_index].respawn_at_ms = Some(retry_at);
                spawn_rules
                    .scheduled_spawns
                    .push(Reverse((retry_at, rule_index)));
            } else {
                spawn_rules.rules[rule_index].respawn_at_ms = None;
            }
        }

        world.resource_mut::<RuntimeState>().is_dirty = true;
    }
}

pub(super) fn preload_map_to_spawn_cap_world(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let rule_count = world
        .entity(map_entity)
        .get::<MapSpawnRules>()
        .map(|r| r.rules.len())
        .unwrap_or_default();

    for rule_index in 0..rule_count {
        loop {
            let (rule, current, target) = {
                let spawn_rules = world.entity(map_entity).get::<MapSpawnRules>();
                let Some(spawn_rules) = spawn_rules else {
                    break;
                };
                let rule_state = &spawn_rules.rules[rule_index];
                (
                    rule_state.rule.clone(),
                    rule_state.entities.len(),
                    rule_state.rule.max_count,
                )
            };
            if current >= target {
                break;
            }

            let mob_ids = {
                let mut state = world.resource_mut::<RuntimeState>();
                expand_spawn_template(&rule.template, &mut state.rng)
            };
            if mob_ids.is_empty() {
                break;
            }

            let mut spawned = 0usize;
            for mob_id in mob_ids {
                let current_count = world
                    .entity(map_entity)
                    .get::<MapSpawnRules>()
                    .map(|r| r.rules[rule_index].entities.len())
                    .unwrap_or(0);
                if current_count >= target {
                    break;
                }

                let Some(entity_id) = spawn_one_mob(world, map_entity, &shared, &rule, mob_id)
                else {
                    continue;
                };

                if let Some(mut spawn_rules) =
                    world.entity_mut(map_entity).get_mut::<MapSpawnRules>()
                {
                    spawn_rules.rules[rule_index].entities.insert(entity_id);
                }

                spawned = spawned.saturating_add(1);
            }

            if spawned == 0 {
                break;
            }
        }
    }

    if let Some(mut spawn_rules) = world.entity_mut(map_entity).get_mut::<MapSpawnRules>() {
        for rule_state in &mut spawn_rules.rules {
            rule_state.respawn_at_ms = None;
        }
        spawn_rules.scheduled_spawns.clear();
    }
}

fn spawn_one_mob(
    world: &mut World,
    map_entity: Entity,
    shared: &SharedConfig,
    rule: &SpawnRule,
    mob_id: MobId,
) -> Option<zohar_domain::entity::EntityId> {
    let proto = shared.mobs.get(&mob_id)?;

    let (entity_id, pos, rot, wander) = {
        let mut state = world.resource_mut::<RuntimeState>();
        let entity_id = next_entity_id(&mut state);
        let pos = rule.area.random_point(&mut state.rng);
        let rot = match rule.facing {
            FacingStrategy::Random => random_protocol_rot(&mut state.rng),
            FacingStrategy::Fixed(direction) => degrees_to_protocol_rot(direction.to_angle()),
        };
        let wander = if proto.mob_kind == MobKind::Monster {
            Some(super::state::MonsterWanderState {
                next_decision_at_ms: state.sim_time_ms.saturating_add(random_idle_decision_delay(
                    &mut state.rng,
                    &shared.monster_wander,
                )),
                pending_wait_at_ms: None,
            })
        } else {
            None
        };
        (entity_id, pos, rot, wander)
    };

    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        spatial.0.insert(entity_id, pos);
    }

    let mut mob_cmd = world.spawn((
        MobMarker,
        NetEntityId { net_id: entity_id },
        MobRef { mob_id },
        super::state::LocalTransform { pos, rot },
    ));
    if let Some(wander) = wander {
        mob_cmd.insert(WanderState(wander));
    }
    let mob_entity = mob_cmd.id();

    world
        .resource_mut::<NetEntityIndex>()
        .0
        .insert(entity_id, mob_entity);

    Some(entity_id)
}
