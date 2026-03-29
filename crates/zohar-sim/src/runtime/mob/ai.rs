use bevy::prelude::*;
use rand::RngExt;
use std::f32::consts::TAU;
use std::sync::Arc;
use zohar_domain::coords::{LocalDistMeters, LocalPos, LocalPosExt, LocalRotation, LocalSize};
use zohar_domain::entity::mob::MobPrototypeDef;
use zohar_domain::entity::{EntityId, MovementKind};

use crate::navigation::MapNavigator;

use super::action_pipeline::{
    Action, ActionBuffer, MobActionCompletion, build_mob_attack_action, build_mob_move_action,
    set_mob_brain,
};
use super::query;
use super::rules::{combat, movement};
use super::state::{
    LocalTransform, MapConfig, MobAggro, MobAggroQueue, MobBrainMode, MobBrainState, MobHomeAnchor,
    MobMotion, MobRef, RuntimeState, SharedConfig,
};
use super::util::{
    clamp_step_towards, packet_time_ms, random_duration_between_ms, rotation_from_delta,
};

const LEGACY_CHASE_FOLLOW_DISTANCE_RATIO: f32 = 0.9;
const LEGACY_ATTACK_THRESHOLD_RATIO: f32 = 1.15;
const HOME_ARRIVAL_RADIUS_M: f32 = 0.75;
const LEGACY_CHASE_RETHINK_MS: u64 = 200;
const CLOSE_CHASE_EPSILON_M: f32 = 0.01;
const IDLE_WANDER_MAX_ATTEMPTS: usize = 8;
const IDLE_WANDER_DISTANCE_EPSILON_M: f32 = 0.25;

struct MobContext {
    proto: Arc<MobPrototypeDef>,
    map_entity: Entity,
    now_ms: u64,
    now_ts: u32,
    current_pos: LocalPos,
    current_rot: u8,
    home_pos: LocalPos,
    movement_in_flight: bool,
    segment_end_pos: Option<LocalPos>,
    segment_end_at_ms: Option<u64>,
}

enum WindupState {
    Blocked,
    Proceed { state_changed: bool },
}

pub(crate) fn mob_follow_distance_m(attack_range_m: f32) -> f32 {
    attack_range_m.max(0.0) * LEGACY_CHASE_FOLLOW_DISTANCE_RATIO
}

pub(crate) fn process_mob_ai(world: &mut World) {
    let mob_entities = {
        let mut query = world.query_filtered::<Entity, With<MobRef>>();
        query.iter(world).collect::<Vec<_>>()
    };
    let mut actions = Vec::new();

    for mob_entity in mob_entities {
        if !world.entities().contains(mob_entity) {
            continue;
        }

        let Some(context) = collect_mob_context(world, mob_entity) else {
            continue;
        };
        let Some(mut brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
            continue;
        };

        let mut state_changed = match handle_attack_windup(&mut brain, context.now_ms) {
            WindupState::Blocked => continue,
            WindupState::Proceed { state_changed } => state_changed,
        };

        let aggros = drain_mob_aggro(world, mob_entity);
        state_changed |= apply_new_aggro(&mut brain, &aggros, context.now_ms);
        state_changed |= acquire_idle_target(world, &context, &mut brain);
        let target_pos = validate_target(world, &context, &mut brain, &mut state_changed);

        let brain_before_decision = brain;
        let planned_action = match (brain.target, brain.mode) {
            (Some(target), _) => {
                let Some(target_pos) = target_pos else {
                    persist_brain_if_changed(world, mob_entity, brain, state_changed);
                    continue;
                };
                handle_pursuit(world, mob_entity, &context, &mut brain, target, target_pos)
            }
            (None, MobBrainMode::Return) => handle_return(world, mob_entity, &context, &mut brain),
            (None, _) => handle_idle(world, mob_entity, &context, &mut brain),
        };
        state_changed |= brain != brain_before_decision;

        if let Some(action) = planned_action {
            actions.push(action);
        } else {
            persist_brain_if_changed(world, mob_entity, brain, state_changed);
        }
    }

    world.resource_mut::<ActionBuffer>().0.extend(actions);
}

fn collect_mob_context(world: &World, mob_entity: Entity) -> Option<MobContext> {
    let mob_ref = world.entity(mob_entity).get::<MobRef>()?;
    let proto = world
        .resource::<SharedConfig>()
        .mobs
        .get(&mob_ref.mob_id)
        .cloned()?;
    let state = world.resource::<RuntimeState>();
    let now_ms = state.sim_time_ms;
    let now_ts = packet_time_ms(state.packet_time_start);
    let map_entity = state.map_entity.unwrap_or(Entity::PLACEHOLDER);
    let current_pos = super::mob_motion::sampled_mob_position(world, mob_entity, now_ms)?;
    let current_rot = world
        .entity(mob_entity)
        .get::<LocalTransform>()
        .map(|transform| transform.rot)?;
    let home_pos = world
        .entity(mob_entity)
        .get::<MobHomeAnchor>()
        .map(|anchor| anchor.pos)
        .unwrap_or(current_pos);
    let movement_in_flight = super::mob_motion::mob_movement_in_flight(world, mob_entity, now_ms);
    let motion = world
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0);

    Some(MobContext {
        proto,
        map_entity,
        now_ms,
        now_ts,
        current_pos,
        current_rot,
        home_pos,
        movement_in_flight,
        segment_end_pos: motion.map(|motion| motion.segment_end_pos),
        segment_end_at_ms: motion.map(|motion| motion.segment_end_at_ms),
    })
}

fn handle_attack_windup(brain: &mut MobBrainState, now_ms: u64) -> WindupState {
    if brain.mode != MobBrainMode::AttackWindup {
        return WindupState::Proceed {
            state_changed: false,
        };
    }
    if now_ms < brain.attack_windup_until_ms {
        return WindupState::Blocked;
    }

    brain.attack_windup_until_ms = 0;
    brain.next_rethink_at_ms = now_ms;
    if brain.target.is_some() {
        brain.mode = MobBrainMode::Pursuit;
    } else {
        transition_to_idle(brain, now_ms);
    }

    WindupState::Proceed {
        state_changed: true,
    }
}

fn apply_new_aggro(brain: &mut MobBrainState, aggros: &[MobAggro], now_ms: u64) -> bool {
    let Some(target_id) = latest_provoked_by(aggros) else {
        return false;
    };
    if brain.target == Some(target_id) && brain.mode == MobBrainMode::Pursuit {
        return false;
    }

    brain.target = Some(target_id);
    brain.mode = MobBrainMode::Pursuit;
    brain.next_rethink_at_ms = now_ms;
    true
}

fn acquire_idle_target(world: &World, context: &MobContext, brain: &mut MobBrainState) -> bool {
    if brain.target.is_some() || brain.mode != MobBrainMode::Idle {
        return false;
    }

    let Some(acquired) = query::acquire_aggressive_target(
        world,
        context.map_entity,
        context.current_pos,
        &context.proto,
    ) else {
        return false;
    };

    brain.target = Some(acquired);
    brain.mode = MobBrainMode::Pursuit;
    brain.next_rethink_at_ms = context.now_ms;
    true
}

fn validate_target(
    world: &World,
    context: &MobContext,
    brain: &mut MobBrainState,
    state_changed: &mut bool,
) -> Option<LocalPos> {
    let target = brain.target?;
    let target_pos = query::player_position(world, target, context.now_ts);
    let target_exceeded_leash = target_pos.is_some_and(|target_pos| {
        query::has_exceeded_leash(context.current_pos, target_pos, context.home_pos)
    });

    if target_pos.is_none() || target_exceeded_leash {
        brain.target = None;
        brain.mode = MobBrainMode::Return;
        *state_changed = true;
        return None;
    }

    target_pos
}

fn handle_return(
    world: &mut World,
    mob_entity: Entity,
    context: &MobContext,
    brain: &mut MobBrainState,
) -> Option<Action> {
    if movement::distance(context.current_pos, context.home_pos) <= HOME_ARRIVAL_RADIUS_M {
        transition_to_idle(brain, context.now_ms);
        return None;
    }

    if context.now_ms < brain.next_rethink_at_ms {
        return None;
    }

    build_mob_move_action(
        world,
        mob_entity,
        MovementKind::Wait,
        context.home_pos,
        context.home_pos,
        *brain,
        MobActionCompletion::RethinkAtActionEndOrDelay {
            max_delay_ms: LEGACY_CHASE_RETHINK_MS,
        },
    )
}

fn handle_idle(
    world: &mut World,
    mob_entity: Entity,
    context: &MobContext,
    brain: &mut MobBrainState,
) -> Option<Action> {
    if let Some(wait_until_ms) = brain.wander_wait_until_ms {
        if context.now_ms >= wait_until_ms {
            brain.wander_wait_until_ms = None;
        } else {
            return None;
        }
    }

    if !context.proto.bhv_flags.can_wander() {
        return None;
    }

    let next_decision_at_ms = if brain.wander_next_decision_at_ms > 0 {
        brain.wander_next_decision_at_ms
    } else {
        context.now_ms
    };
    if context.now_ms < next_decision_at_ms {
        return None;
    }

    let shared = world.resource::<SharedConfig>().clone();
    let pause_until = {
        let mut state = world.resource_mut::<RuntimeState>();
        context.now_ms.saturating_add(random_duration_between_ms(
            &mut state.rng,
            shared.wander.decision_pause_idle_min,
            shared.wander.decision_pause_idle_max,
        ))
    };

    let should_wander = {
        let mut state = world.resource_mut::<RuntimeState>();
        let denom = shared.wander.wander_chance_denominator.max(1);
        state.rng.random_range(0..denom) == 0
    };
    if !should_wander {
        brain.wander_next_decision_at_ms = pause_until;
        brain.wander_wait_until_ms = None;
        return None;
    }

    let (map_size, navigator) = {
        let map = world.resource::<MapConfig>();
        (map.local_size, map.navigator.clone())
    };
    let destination = {
        let mut state = world.resource_mut::<RuntimeState>();
        sample_idle_wander_target(
            &mut state.rng,
            map_size,
            navigator.as_deref(),
            context.current_pos,
            context.current_rot,
            shared.wander.step_min_m,
            shared.wander.step_max_m,
        )
    };

    let Some((destination, _)) = destination else {
        brain.wander_next_decision_at_ms = pause_until;
        return None;
    };

    let post_move_pause_ms = {
        let mut state = world.resource_mut::<RuntimeState>();
        random_duration_between_ms(
            &mut state.rng,
            shared.wander.post_move_pause_min,
            shared.wander.post_move_pause_max,
        )
    };

    build_mob_move_action(
        world,
        mob_entity,
        MovementKind::Wait,
        destination,
        destination,
        *brain,
        MobActionCompletion::IdleWander { post_move_pause_ms },
    )
    .or_else(|| {
        brain.wander_next_decision_at_ms = pause_until;
        None
    })
}

fn handle_pursuit(
    world: &mut World,
    mob_entity: Entity,
    context: &MobContext,
    brain: &mut MobBrainState,
    target: EntityId,
    target_pos: LocalPos,
) -> Option<Action> {
    let chase_target_pos = query::chase_target_position(
        world,
        context.current_pos,
        target,
        context.now_ts,
        context.proto.mob_id,
        context.proto.move_speed,
        context.proto.battle_type,
        world.resource::<SharedConfig>(),
    )
    .unwrap_or(target_pos);

    let attack_range_m =
        combat::effective_attack_range_m(context.proto.attack_range, context.proto.battle_type);
    let follow_distance_m = mob_follow_distance_m(attack_range_m);
    let attack_threshold_m = attack_range_m.max(0.0) * LEGACY_ATTACK_THRESHOLD_RATIO;
    let target_distance_m = movement::distance(context.current_pos, target_pos);
    let segment_end_in_attack_range = context.segment_end_pos.is_some_and(|segment_end_pos| {
        movement::distance(segment_end_pos, target_pos) <= attack_threshold_m
    });

    if context.now_ms >= brain.next_attack_at_ms
        && target_distance_m <= attack_threshold_m
        && (!context.movement_in_flight || segment_end_in_attack_range)
    {
        let timing = combat::attack_timing_for_mob(context.proto.attack_speed);
        brain.mode = MobBrainMode::AttackWindup;
        brain.attack_windup_until_ms = context
            .now_ms
            .saturating_add(u64::from(timing.packet_duration_ms));
        brain.next_attack_at_ms = context.now_ms.saturating_add(timing.cooldown_ms);

        return build_mob_attack_action(
            world,
            mob_entity,
            target_pos,
            timing.packet_duration_ms,
            *brain,
            MobActionCompletion::RethinkAtActionEnd,
        );
    }

    if context.movement_in_flight && segment_end_in_attack_range {
        brain.next_rethink_at_ms = context.segment_end_at_ms.unwrap_or(context.now_ms);
        return None;
    }
    if context.now_ms < brain.next_rethink_at_ms {
        return None;
    }

    let chase_distance_m = movement::distance(context.current_pos, chase_target_pos);
    let step_len = (chase_distance_m - follow_distance_m).max(0.0);
    if step_len <= CLOSE_CHASE_EPSILON_M {
        brain.next_rethink_at_ms = context.now_ms.saturating_add(LEGACY_CHASE_RETHINK_MS);
        return None;
    }

    let destination = clamp_step_towards(context.current_pos, chase_target_pos, step_len);
    if movement::desired_destination_unchanged(context.segment_end_pos, destination) {
        brain.next_rethink_at_ms = context.now_ms.saturating_add(LEGACY_CHASE_RETHINK_MS);
        return None;
    }

    build_mob_move_action(
        world,
        mob_entity,
        MovementKind::Wait,
        destination,
        chase_target_pos,
        *brain,
        MobActionCompletion::RethinkAtActionEndOrDelay {
            max_delay_ms: LEGACY_CHASE_RETHINK_MS,
        },
    )
}

fn persist_brain_if_changed(
    world: &mut World,
    mob_entity: Entity,
    brain: MobBrainState,
    state_changed: bool,
) {
    if state_changed {
        set_mob_brain(world, mob_entity, brain);
    }
}

fn transition_to_idle(brain: &mut MobBrainState, now_ms: u64) {
    brain.mode = MobBrainMode::Idle;
    brain.target = None;
    brain.attack_windup_until_ms = 0;
    brain.next_rethink_at_ms = 0;
    brain.wander_next_decision_at_ms = brain.wander_next_decision_at_ms.max(now_ms);
    brain.wander_wait_until_ms = None;
}

fn drain_mob_aggro(world: &mut World, mob_entity: Entity) -> Vec<MobAggro> {
    let mut entity = world.entity_mut(mob_entity);
    let Some(mut queue) = entity.get_mut::<MobAggroQueue>() else {
        return Vec::new();
    };
    std::mem::take(&mut queue.0)
}

fn latest_provoked_by(aggros: &[MobAggro]) -> Option<EntityId> {
    aggros.iter().rev().find_map(|aggro| match aggro {
        MobAggro::ProvokedBy { attacker } => Some(*attacker),
    })
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
        if movement::distance(current_pos, candidate) <= IDLE_WANDER_DISTANCE_EPSILON_M {
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
