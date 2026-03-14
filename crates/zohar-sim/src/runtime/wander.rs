use bevy::prelude::*;
use rand::RngExt;
use std::f32::consts::TAU;
use zohar_domain::coords::{LocalDistMeters, LocalPos, LocalPosExt, LocalRotation, LocalSize};
use zohar_domain::entity::MovementKind;

use crate::motion::MotionMoveMode;
use crate::navigation::MapNavigator;

use super::state::{
    LocalTransform, MapConfig, MapPendingMovements, MapSpatial, MobRef, NetEntityId,
    NetEntityIndex, PendingMovement, RuntimeState, SharedConfig, WanderState, WanderStateData,
};
use super::util::{
    calculate_mob_move_duration_ms, packet_time_ms, random_duration_between_ms, rotation_from_delta,
};

pub(super) fn monster_wander(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let map_config = world.resource::<MapConfig>();
    let validator = WanderValidator::new(map_config.local_size, map_config.navigator.clone());

    let (now_ms, now_ts) = {
        let state = world.resource::<RuntimeState>();
        (state.sim_time_ms, packet_time_ms(state.packet_time_start))
    };

    let mob_entities: Vec<zohar_domain::entity::EntityId> = {
        let mut all_mobs = world.query::<(&MobRef, &NetEntityId)>();
        all_mobs
            .iter(world)
            .map(|(_, net_entity_id)| net_entity_id.net_id)
            .collect()
    };

    let mut dirty_map = false;

    for mob_net_id in mob_entities {
        let mob_entity = {
            let net_index = world.resource::<NetEntityIndex>();
            net_index.0.get(&mob_net_id).copied()
        };
        let Some(mob_entity) = mob_entity else {
            continue;
        };

        let (mob_id, old_pos, current_rot, mut wander) = {
            let mob_ref = world.entity(mob_entity).get::<MobRef>();
            let transform = world.entity(mob_entity).get::<LocalTransform>();
            let wander_state = world.entity(mob_entity).get::<WanderState>();
            let (Some(mob_ref), Some(transform), Some(wander_state)) =
                (mob_ref, transform, wander_state)
            else {
                continue;
            };
            (mob_ref.mob_id, transform.pos, transform.rot, wander_state.0)
        };

        let Some(proto) = shared.mobs.get(&mob_id) else {
            continue;
        };
        if !proto.bhv_flags.can_wander() {
            continue;
        }

        if let Some(wait_at_ms) = wander.pending_wait_at_ms {
            if now_ms >= wait_at_ms {
                wander.pending_wait_at_ms = None;
                store_wander_state(world, mob_entity, wander);
            }
            continue;
        }

        if now_ms < wander.next_decision_at_ms {
            continue;
        }

        let should_wander = {
            let mut state = world.resource_mut::<RuntimeState>();
            let chance_denom = shared.wander.wander_chance_denominator.max(1);
            state.rng.random_range(0..chance_denom) == 0
        };

        if !should_wander {
            reschedule_idle_decision(world, mob_entity, &mut wander, now_ms, &shared);
            continue;
        }

        let Some((new_pos, rot)) = ({
            let mut state = world.resource_mut::<RuntimeState>();
            sample_idle_wander_target(
                &mut state.rng,
                &validator,
                old_pos,
                current_rot,
                shared.wander.step_min_m,
                shared.wander.step_max_m,
            )
        }) else {
            reschedule_idle_decision(world, mob_entity, &mut wander, now_ms, &shared);
            continue;
        };

        let duration = calculate_mob_move_duration_ms(
            &shared.motion_speeds,
            mob_id,
            MotionMoveMode::Run,
            proto.move_speed,
            old_pos,
            new_pos,
        );

        if duration == 0 {
            reschedule_idle_decision(world, mob_entity, &mut wander, now_ms, &shared);
            continue;
        }

        let (wait_at_ms, next_decision_at_ms) = {
            let mut state = world.resource_mut::<RuntimeState>();
            let wait_at = now_ms.saturating_add(duration as u64);
            let next_decision = wait_at.saturating_add(random_duration_between_ms(
                &mut state.rng,
                shared.wander.post_move_pause_min,
                shared.wander.post_move_pause_max,
            ));
            (wait_at, next_decision)
        };

        wander.pending_wait_at_ms = Some(wait_at_ms);
        wander.next_decision_at_ms = next_decision_at_ms;

        if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
            spatial.0.update_position(mob_net_id, new_pos);
        }
        if let Some(mut transform) = world.entity_mut(mob_entity).get_mut::<LocalTransform>() {
            transform.pos = new_pos;
            transform.rot = rot;
        }
        store_wander_state(world, mob_entity, wander);
        if let Some(mut pending) = world
            .entity_mut(map_entity)
            .get_mut::<MapPendingMovements>()
        {
            pending.0.push(PendingMovement {
                mover_player_id: None,
                entity_id: mob_net_id,
                new_pos,
                kind: MovementKind::Wait,
                arg: 0,
                rot,
                ts: now_ts,
                duration,
            });
        }
        dirty_map = true;
    }

    if dirty_map {
        world.resource_mut::<RuntimeState>().is_dirty = true;
    }
}

fn reschedule_idle_decision(
    world: &mut World,
    mob_entity: Entity,
    wander: &mut WanderStateData,
    now_ms: u64,
    shared: &SharedConfig,
) {
    let mut state = world.resource_mut::<RuntimeState>();
    wander.next_decision_at_ms = now_ms.saturating_add(random_duration_between_ms(
        &mut state.rng,
        shared.wander.decision_pause_idle_min,
        shared.wander.decision_pause_idle_max,
    ));
    store_wander_state(world, mob_entity, *wander);
}

fn store_wander_state(world: &mut World, mob_entity: Entity, wander: WanderStateData) {
    if let Some(mut ws) = world.entity_mut(mob_entity).get_mut::<WanderState>() {
        ws.0 = wander;
    }
}

fn sample_idle_wander_target(
    rng: &mut rand::rngs::SmallRng,
    validator: &WanderValidator,
    current_pos: LocalPos,
    current_rot: u8,
    step_min_m: f32,
    step_max_m: f32,
) -> Option<(LocalPos, u8)> {
    let step_min = step_min_m.min(step_max_m);
    let step_max = step_min_m.max(step_max_m);
    let heading = LocalRotation::radians(rng.random_range(0.0..TAU));
    let distance = LocalDistMeters::new(rng.random_range(step_min..=step_max));
    let candidate = current_pos.shifted(heading, distance);

    if !validator.is_allowed(current_pos, candidate) {
        return None;
    }

    let rot = rotation_from_delta(current_pos, candidate, current_rot);
    Some((candidate, rot))
}

#[derive(Clone)]
struct WanderValidator {
    map_size: LocalSize,
    navigator: Option<std::sync::Arc<MapNavigator>>,
}

impl WanderValidator {
    fn new(map_size: LocalSize, navigator: Option<std::sync::Arc<MapNavigator>>) -> Self {
        Self {
            map_size,
            navigator,
        }
    }

    fn is_allowed(&self, current_pos: LocalPos, candidate: LocalPos) -> bool {
        self.contains_local(candidate)
            && self
                .navigator
                .as_ref()
                .is_none_or(|nav| nav.segment_clear(current_pos, candidate))
    }

    fn contains_local(&self, pos: LocalPos) -> bool {
        pos.x.is_finite()
            && pos.y.is_finite()
            && pos.x >= 0.0
            && pos.y >= 0.0
            && pos.x < self.map_size.width
            && pos.y < self.map_size.height
    }
}

#[cfg(test)]
pub(super) fn is_wander_target_allowed(
    map_size: LocalSize,
    current_pos: LocalPos,
    candidate: LocalPos,
) -> bool {
    WanderValidator::new(map_size, None).is_allowed(current_pos, candidate)
}

#[cfg(test)]
pub(super) fn is_wander_target_allowed_with_collision(
    map_size: LocalSize,
    current_pos: LocalPos,
    candidate: LocalPos,
    navigator: std::sync::Arc<MapNavigator>,
) -> bool {
    WanderValidator::new(map_size, Some(navigator)).is_allowed(current_pos, candidate)
}
