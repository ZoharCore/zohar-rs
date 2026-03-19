use bevy::prelude::*;
use rand::RngExt;
use std::f32::consts::TAU;
use zohar_domain::coords::{LocalDistMeters, LocalPos, LocalPosExt, LocalRotation, LocalSize};

use crate::navigation::MapNavigator;

use super::mob_motion::issue_mob_action;
use super::state::{
    LocalTransform, MapConfig, MapSpatial, MobBrainMode, MobBrainState, MobHomeAnchor, MobPackId,
    MobRef, NetEntityId, NetEntityIndex, PlayerMarker, RuntimeState, SharedConfig, WanderConfig,
};
use super::util::{packet_time_ms, random_duration_between_ms, rotation_from_delta};
use zohar_domain::entity::MovementKind;

const HOME_ARRIVAL_RADIUS_M: f32 = 0.75;
const MOB_LEASH_DISTANCE_M: Option<f32> = None;
const MOB_LOCK_FREEZE_MS: u64 = 3_000;
const IDLE_WANDER_MAX_ATTEMPTS: usize = 8;
const IDLE_WANDER_DISTANCE_EPSILON_M: f32 = 0.25;

pub(super) fn mob_brain_tick(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let now_ms = world.resource::<RuntimeState>().sim_time_ms;
    let now_ts = packet_time_ms(world.resource::<RuntimeState>().packet_time_start);
    let mob_entities = mob_entities(world);

    for mob_entity in mob_entities {
        if !world.entities().contains(mob_entity) {
            continue;
        }

        let Some(mob_ref) = world.entity(mob_entity).get::<MobRef>() else {
            continue;
        };
        let Some(proto) = shared.mobs.get(&mob_ref.mob_id) else {
            continue;
        };
        let Some((mob_net_id, current_pos, current_rot)) = world
            .entity(mob_entity)
            .get::<NetEntityId>()
            .zip(world.entity(mob_entity).get::<LocalTransform>())
            .map(|(net_id, transform)| (net_id.net_id, transform.pos, transform.rot))
        else {
            continue;
        };
        let home_pos = world
            .entity(mob_entity)
            .get::<MobHomeAnchor>()
            .map(|anchor| anchor.pos)
            .unwrap_or(current_pos);

        maybe_acquire_aggressive_target(world, map_entity, mob_entity, current_pos, proto, now_ms);

        let Some(brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
            continue;
        };
        if brain.mode == MobBrainMode::Attacking {
            if now_ms < brain.attack_windup_until_ms {
                continue;
            }
            if let Some(mut brain_mut) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
                brain_mut.attack_windup_until_ms = 0;
                brain_mut.next_chase_rethink_at_ms = now_ms;
                if brain_mut.target.is_some() {
                    brain_mut.mode = MobBrainMode::Chasing;
                } else {
                    *brain_mut = idle_brain_state(*brain_mut, now_ms);
                }
            }
        }

        let Some(brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
            continue;
        };
        if let Some(target) = brain.target {
            let Some(target_pos) = player_position(world, target) else {
                start_returning(world, mob_entity, current_pos, home_pos, now_ms);
                continue;
            };
            if has_exceeded_leash(current_pos, target_pos, home_pos) {
                start_returning(world, mob_entity, current_pos, home_pos, now_ms);
                continue;
            }

            if let Some(mut brain_mut) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
                brain_mut.mode = MobBrainMode::Chasing;
                brain_mut.next_chase_rethink_at_ms = brain_mut.next_chase_rethink_at_ms.min(now_ms);
            }
            continue;
        }

        if brain.mode == MobBrainMode::Returning {
            if distance(current_pos, home_pos) <= HOME_ARRIVAL_RADIUS_M {
                if let Some(mut brain_mut) = world.entity_mut(mob_entity).get_mut::<MobBrainState>()
                {
                    *brain_mut = idle_brain_state(*brain_mut, now_ms);
                }
            }
            continue;
        }

        if proto.bhv_flags.can_wander() {
            maybe_issue_idle_wander(
                world,
                map_entity,
                mob_entity,
                mob_net_id,
                current_pos,
                current_rot,
                &shared,
                now_ms,
                now_ts,
            );
        }
    }
}

pub(super) fn retaliate_mob_pack(
    world: &mut World,
    attacked_mob_entity: Entity,
    target: zohar_domain::entity::EntityId,
    now_ms: u64,
) {
    let attacked_pack_id = world
        .entity(attacked_mob_entity)
        .get::<MobPackId>()
        .copied()
        .map(|pack| pack.pack_id);

    if let Some(pack_id) = attacked_pack_id {
        for mob_entity in mob_pack_members(world, pack_id) {
            lock_mob_target(world, mob_entity, target, now_ms, true);
        }
    } else {
        lock_mob_target(world, attacked_mob_entity, target, now_ms, true);
    }
}

fn maybe_acquire_aggressive_target(
    world: &mut World,
    map_entity: Entity,
    mob_entity: Entity,
    current_pos: LocalPos,
    proto: &zohar_domain::entity::mob::MobPrototype,
    now_ms: u64,
) {
    let Some(brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
        return;
    };
    if brain.target.is_some() || brain.mode != MobBrainMode::Idle {
        return;
    }
    if !proto
        .bhv_flags
        .contains(zohar_domain::BehaviorFlags::AGGRESSIVE)
    {
        return;
    }
    let sight_m = proto.aggressive_sight as f32 / 100.0;
    if sight_m <= 0.0 {
        return;
    }
    let Some(target) = find_nearest_player_in_radius(world, map_entity, current_pos, sight_m)
    else {
        return;
    };
    lock_mob_target(world, mob_entity, target, now_ms, false);
}

#[allow(clippy::too_many_arguments)]
fn maybe_issue_idle_wander(
    world: &mut World,
    map_entity: Entity,
    mob_entity: Entity,
    mob_net_id: zohar_domain::entity::EntityId,
    current_pos: LocalPos,
    current_rot: u8,
    shared: &SharedConfig,
    now_ms: u64,
    now_ts: u32,
) {
    let Some(mut brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
        return;
    };
    if brain.mode != MobBrainMode::Idle {
        return;
    }

    if let Some(wait_until_ms) = brain.wander_wait_until_ms {
        if now_ms >= wait_until_ms {
            brain.wander_wait_until_ms = None;
        } else {
            return;
        }
    }
    if now_ms < brain.next_wander_decision_at_ms {
        return;
    }

    let should_wander = {
        let mut state = world.resource_mut::<RuntimeState>();
        let denom = shared.wander.wander_chance_denominator.max(1);
        state.rng.random_range(0..denom) == 0
    };
    if !should_wander {
        reschedule_idle_wander(world, mob_entity, brain, &shared.wander, now_ms);
        return;
    }

    let (map_size, navigator) = {
        let map = world.resource::<MapConfig>();
        (map.local_size, map.navigator.clone())
    };
    let Some((new_pos, rot)) = ({
        let mut state = world.resource_mut::<RuntimeState>();
        sample_idle_wander_target(
            &mut state.rng,
            map_size,
            navigator.as_deref(),
            current_pos,
            current_rot,
            shared.wander.step_min_m,
            shared.wander.step_max_m,
        )
    }) else {
        reschedule_idle_wander(world, mob_entity, brain, &shared.wander, now_ms);
        return;
    };

    let Some(mob_ref) = world.entity(mob_entity).get::<MobRef>() else {
        return;
    };
    let Some(proto) = shared.mobs.get(&mob_ref.mob_id) else {
        return;
    };
    let Some(duration) = issue_mob_action(
        world,
        map_entity,
        mob_entity,
        mob_net_id,
        MovementKind::Wait,
        current_pos,
        new_pos,
        rot,
        mob_ref.mob_id,
        proto.move_speed,
        shared,
        now_ms,
        now_ts,
        None,
    ) else {
        reschedule_idle_wander(world, mob_entity, brain, &shared.wander, now_ms);
        return;
    };

    let next_decision_at_ms = {
        let mut state = world.resource_mut::<RuntimeState>();
        now_ms
            .saturating_add(u64::from(duration))
            .saturating_add(random_duration_between_ms(
                &mut state.rng,
                shared.wander.post_move_pause_min,
                shared.wander.post_move_pause_max,
            ))
    };
    brain.wander_wait_until_ms = Some(now_ms.saturating_add(u64::from(duration)));
    brain.next_wander_decision_at_ms = next_decision_at_ms;
    if let Some(mut brain_state) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
        *brain_state = brain;
    }
}

fn reschedule_idle_wander(
    world: &mut World,
    mob_entity: Entity,
    mut brain: MobBrainState,
    wander: &WanderConfig,
    now_ms: u64,
) {
    let next_decision_at_ms = {
        let mut state = world.resource_mut::<RuntimeState>();
        now_ms.saturating_add(random_duration_between_ms(
            &mut state.rng,
            wander.decision_pause_idle_min,
            wander.decision_pause_idle_max,
        ))
    };
    brain.next_wander_decision_at_ms = next_decision_at_ms;
    if let Some(mut brain_state) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
        *brain_state = brain;
    }
}

fn lock_mob_target(
    world: &mut World,
    mob_entity: Entity,
    target: zohar_domain::entity::EntityId,
    now_ms: u64,
    force_retarget: bool,
) {
    let Some(current) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
        return;
    };
    if !force_retarget
        && current.target.is_some_and(|existing| {
            existing != target
                && now_ms.saturating_sub(current.target_locked_at_ms) < MOB_LOCK_FREEZE_MS
        })
    {
        return;
    }

    if let Some(mut brain) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
        brain.target = Some(target);
        brain.target_locked_at_ms = now_ms;
        brain.next_chase_rethink_at_ms = now_ms;
        brain.attack_windup_until_ms = 0;
        brain.mode = MobBrainMode::Chasing;
    }
}

fn start_returning(
    world: &mut World,
    mob_entity: Entity,
    current_pos: LocalPos,
    home_pos: LocalPos,
    now_ms: u64,
) {
    let mut entity_mut = world.entity_mut(mob_entity);
    let Some(mut brain) = entity_mut.get_mut::<MobBrainState>() else {
        return;
    };
    brain.target = None;
    brain.target_locked_at_ms = 0;
    brain.attack_windup_until_ms = 0;
    brain.next_chase_rethink_at_ms = now_ms;
    if distance(current_pos, home_pos) <= HOME_ARRIVAL_RADIUS_M {
        *brain = idle_brain_state(*brain, now_ms);
    } else {
        brain.mode = MobBrainMode::Returning;
    }
}

fn idle_brain_state(mut brain: MobBrainState, now_ms: u64) -> MobBrainState {
    brain.mode = MobBrainMode::Idle;
    brain.target = None;
    brain.target_locked_at_ms = 0;
    brain.attack_windup_until_ms = 0;
    brain.next_chase_rethink_at_ms = 0;
    brain.next_wander_decision_at_ms = brain.next_wander_decision_at_ms.max(now_ms);
    brain.wander_wait_until_ms = None;
    brain
}

fn find_nearest_player_in_radius(
    world: &World,
    map_entity: Entity,
    center: LocalPos,
    radius: f32,
) -> Option<zohar_domain::entity::EntityId> {
    let spatial = world.entity(map_entity).get::<MapSpatial>()?;
    spatial
        .0
        .query_in_radius(center, radius)
        .filter_map(|candidate| {
            let entity = net_entity(world, candidate)?;
            world.entity(entity).get::<PlayerMarker>()?;
            let pos = world.entity(entity).get::<LocalTransform>()?.pos;
            Some((candidate, distance(center, pos)))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(candidate, _)| candidate)
}

fn player_position(world: &World, entity_id: zohar_domain::entity::EntityId) -> Option<LocalPos> {
    let entity = net_entity(world, entity_id)?;
    world.entity(entity).get::<PlayerMarker>()?;
    world
        .entity(entity)
        .get::<LocalTransform>()
        .map(|transform| transform.pos)
}

fn net_entity(world: &World, entity_id: zohar_domain::entity::EntityId) -> Option<Entity> {
    world
        .resource::<NetEntityIndex>()
        .0
        .get(&entity_id)
        .copied()
}

fn mob_entities(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<Entity, With<MobRef>>();
    query.iter(world).collect()
}

fn mob_pack_members(world: &mut World, pack_id: u32) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &MobPackId)>();
    query
        .iter(world)
        .filter_map(|(entity, mob_pack_id)| (mob_pack_id.pack_id == pack_id).then_some(entity))
        .collect()
}

fn distance(from: LocalPos, to: LocalPos) -> f32 {
    (to - from).length()
}

fn has_exceeded_leash(current_pos: LocalPos, target_pos: LocalPos, home_pos: LocalPos) -> bool {
    let Some(leash_distance_m) = MOB_LEASH_DISTANCE_M else {
        return false;
    };
    distance(current_pos, home_pos) > leash_distance_m
        || distance(target_pos, home_pos) > leash_distance_m
}

fn sample_idle_wander_target(
    rng: &mut rand::rngs::SmallRng,
    map_size: LocalSize,
    navigator: Option<&MapNavigator>,
    current_pos: LocalPos,
    current_rot: u8,
    step_min_m: f32,
    step_max_m: f32,
) -> Option<(LocalPos, u8)> {
    let step_min = step_min_m.min(step_max_m);
    let step_max = step_min_m.max(step_max_m);
    for _ in 0..IDLE_WANDER_MAX_ATTEMPTS {
        let heading = LocalRotation::radians(rng.random_range(0.0..TAU));
        let step_distance = LocalDistMeters::new(rng.random_range(step_min..=step_max));
        let candidate = current_pos.shifted(heading, step_distance);
        if !is_inside_map(map_size, candidate) {
            continue;
        }
        if navigator.is_some_and(|nav| {
            !nav.can_stand(candidate)
                || !nav.same_component(current_pos, candidate)
                || !nav.segment_clear(current_pos, candidate)
        }) {
            continue;
        }
        if distance(current_pos, candidate) <= IDLE_WANDER_DISTANCE_EPSILON_M {
            continue;
        }
        let rot = rotation_from_delta(current_pos, candidate, current_rot);
        return Some((candidate, rot));
    }

    None
}

fn is_inside_map(map_size: LocalSize, candidate: LocalPos) -> bool {
    candidate.x.is_finite()
        && candidate.y.is_finite()
        && candidate.x >= 0.0
        && candidate.y >= 0.0
        && candidate.x < map_size.width
        && candidate.y < map_size.height
}
