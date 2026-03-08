use super::state::{
    LocalTransform, MapEmpire, MapMarker, MapPendingLocalChats, MapPendingMovements,
    MapReplication, MapSpatial, MapSpawnRules, MobMarker, MobRef, NetEntityId, NetEntityIndex,
    RuntimeState, SharedConfig, SpawnRuleState, StartupReadySignal, WanderState, WanderStateData,
};
use super::util::{
    degrees_to_protocol_rot, expand_spawn_template, next_entity_id, random_duration_between_ms,
    random_protocol_rot,
};
use bevy::prelude::*;
use rand::RngExt;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use zohar_domain::coords::{LocalBox, LocalBoxExt, LocalPos, LocalSize};
use zohar_domain::entity::mob::MobId;
use zohar_domain::entity::mob::spawn::{FacingStrategy, SpawnRule};

pub(super) fn bootstrap_map_runtime(world: &mut World) {
    let map_config = world.resource::<super::state::MapConfig>();
    let sim_time_ms = world.resource::<RuntimeState>().sim_time_ms;

    let mut rules = Vec::new();
    let mut scheduled_spawns = BinaryHeap::new();
    for (idx, rule) in map_config.spawn_rules.iter().cloned().enumerate() {
        rules.push(SpawnRuleState {
            rule,
            active_instances: 0,
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
                        } else if rule_state.active_instances >= rule_state.rule.max_count {
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
                spawn_rules.rules[rule_index].active_instances,
            )
        };

        let remaining_for_rule = rule.max_count.saturating_sub(existing_count);
        let mut remaining_for_rule_tick = remaining_for_rule;

        while remaining_for_rule_tick > 0 {
            let before_group_budget = remaining_for_rule_tick;
            let spawned_entities = spawn_one_template(world, map_entity, &shared, &rule);
            if spawned_entities.is_empty() {
                break;
            }

            let mut map_ent = world.entity_mut(map_entity);
            let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() else {
                continue;
            };
            let rule_state = &mut spawn_rules.rules[rule_index];
            rule_state.entities.extend(spawned_entities);
            rule_state.active_instances = rule_state.active_instances.saturating_add(1);
            remaining_for_rule_tick = remaining_for_rule_tick.saturating_sub(1);

            if remaining_for_rule_tick == before_group_budget {
                break;
            }
        }

        {
            let mut map_ent = world.entity_mut(map_entity);
            let Some(mut spawn_rules) = map_ent.get_mut::<MapSpawnRules>() else {
                continue;
            };
            let current = spawn_rules.rules[rule_index].active_instances;
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
                    rule_state.active_instances,
                    rule_state.rule.max_count,
                )
            };
            if current >= target {
                break;
            }

            let current_count = world
                .entity(map_entity)
                .get::<MapSpawnRules>()
                .map(|r| r.rules[rule_index].active_instances)
                .unwrap_or(0);
            if current_count >= target {
                break;
            }

            let spawned_entities = spawn_one_template(world, map_entity, &shared, &rule);
            if spawned_entities.is_empty() {
                break;
            }

            let mut spawned: usize = 0;
            if let Some(mut spawn_rules) = world.entity_mut(map_entity).get_mut::<MapSpawnRules>() {
                let rule_state = &mut spawn_rules.rules[rule_index];
                rule_state.entities.extend(spawned_entities);
                rule_state.active_instances = rule_state.active_instances.saturating_add(1);
                spawned = 1;
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

fn spawn_one_template(
    world: &mut World,
    map_entity: Entity,
    shared: &SharedConfig,
    rule: &SpawnRule,
) -> Vec<zohar_domain::entity::EntityId> {
    let map_bounds = map_local_bounds(world.resource::<super::state::MapConfig>().local_size);
    let mob_ids = {
        let mut state = world.resource_mut::<RuntimeState>();
        expand_spawn_template(&rule.template, &mut state.rng)
    };
    let mut spawned_entities = Vec::with_capacity(mob_ids.len());
    let mut previous_pos = None;

    for mob_id in mob_ids {
        let Some((pos, rot)) = ({
            let mut state = world.resource_mut::<RuntimeState>();
            let allowed_bounds = match previous_pos {
                Some(prev_pos) => {
                    let chained_bounds = chained_group_member_bounds(prev_pos, &mut state);
                    rule.area
                        .bounds
                        .intersect(chained_bounds)
                        .and_then(|bounds| bounds.intersect(map_bounds))
                }
                None => rule.area.bounds.intersect(map_bounds),
            };
            let pos = allowed_bounds.map(|bounds| bounds.sample_pos(&mut state.rng));
            let rot = match rule.facing {
                FacingStrategy::Random => random_protocol_rot(&mut state.rng),
                FacingStrategy::Fixed(direction) => degrees_to_protocol_rot(direction.to_angle()),
            };
            pos.map(|pos| (pos, rot))
        }) else {
            if previous_pos.is_none() {
                return Vec::new();
            }
            continue;
        };

        let Some(entity_id) = spawn_one_mob(world, map_entity, shared, rule, mob_id, pos, rot)
        else {
            if previous_pos.is_none() {
                return Vec::new();
            }
            continue;
        };

        spawned_entities.push(entity_id);
        previous_pos = Some(pos);
    }

    spawned_entities
}

fn spawn_one_mob(
    world: &mut World,
    map_entity: Entity,
    shared: &SharedConfig,
    _rule: &SpawnRule,
    mob_id: MobId,
    pos: LocalPos,
    rot: u8,
) -> Option<zohar_domain::entity::EntityId> {
    let proto = shared.mobs.get(&mob_id)?;

    let (entity_id, wander) = {
        let mut state = world.resource_mut::<RuntimeState>();
        let entity_id = next_entity_id(&mut state);
        let wander = proto.bhv_flags.can_wander().then(|| WanderStateData {
            next_decision_at_ms: state.sim_time_ms.saturating_add(random_duration_between_ms(
                &mut state.rng,
                shared.wander.decision_pause_idle_min,
                shared.wander.decision_pause_idle_max,
            )),
            pending_wait_at_ms: None,
        });

        (entity_id, wander)
    };

    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        spatial.0.insert(entity_id, pos);
    }

    let mut mob_cmd = world.spawn((
        MobMarker,
        NetEntityId { net_id: entity_id },
        MobRef { mob_id },
        LocalTransform { pos, rot },
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

fn chained_group_member_bounds(pos: LocalPos, state: &mut RuntimeState) -> LocalBox {
    let extent_x = state.rng.random_range(3.0..=5.0);
    let extent_y = state.rng.random_range(3.0..=5.0);
    LocalBox::from_center_half_extent(pos, LocalSize::new(extent_x, extent_y))
}

fn map_local_bounds(local_size: LocalSize) -> LocalBox {
    LocalBox::new(
        LocalPos::new(0.0, 0.0),
        LocalPos::new(local_size.width, local_size.height),
    )
}
