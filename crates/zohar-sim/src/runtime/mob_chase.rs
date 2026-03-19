use bevy::prelude::*;
use std::sync::Arc;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::mob::MobBattleType;

use crate::navigation::MapNavigator;

use super::mob_motion::{issue_mob_action, mob_movement_in_flight, sampled_mob_position};
use super::state::{
    LocalTransform, MapConfig, MobBrainMode, MobBrainState, MobHomeAnchor, MobMotion, MobRef,
    NetEntityId, NetEntityIndex, PlayerMarker, RuntimeState, SharedConfig,
};
use super::util::{clamp_step_towards, packet_time_ms, rotation_from_delta};

const LEGACY_CHASE_RETHINK_MS: u64 = 200;
const LEGACY_CHASE_FOLLOW_DISTANCE_RATIO: f32 = 0.9;
const LEGACY_ATTACK_THRESHOLD_RATIO: f32 = 1.15;
const HOME_ARRIVAL_RADIUS_M: f32 = 0.75;
const CLOSE_CHASE_EPSILON_M: f32 = 0.01;
const DESTINATION_EPSILON_M: f32 = 0.05;
const ROUTED_PLAN_HORIZON_M: f32 = 20.0;

#[derive(Debug, Clone, Copy)]
struct AttackTiming {
    cooldown_ms: u64,
    packet_duration_ms: u32,
}

pub(super) fn mob_chase_tick(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let navigator = world.resource::<MapConfig>().navigator.clone();
    let now_ms = world.resource::<RuntimeState>().sim_time_ms;
    let now_ts = packet_time_ms(world.resource::<RuntimeState>().packet_time_start);
    let mob_entities = mob_entities(world);

    for mob_entity in mob_entities {
        if !world.entities().contains(mob_entity) {
            continue;
        }

        let Some(brain) = world.entity(mob_entity).get::<MobBrainState>().copied() else {
            continue;
        };
        if brain.mode == MobBrainMode::Attacking {
            continue;
        }

        let Some(mob_ref) = world.entity(mob_entity).get::<MobRef>() else {
            continue;
        };
        let Some(proto) = shared.mobs.get(&mob_ref.mob_id) else {
            continue;
        };
        let Some((mob_net_id, current_rot)) = world
            .entity(mob_entity)
            .get::<NetEntityId>()
            .zip(world.entity(mob_entity).get::<LocalTransform>())
            .map(|(net_id, transform)| (net_id.net_id, transform.rot))
        else {
            continue;
        };
        let Some(current_pos) = sampled_mob_position(world, mob_entity, now_ms) else {
            continue;
        };

        match brain.mode {
            MobBrainMode::Chasing => {
                let Some(target) = brain.target else {
                    continue;
                };
                let Some(target_pos) = player_position(world, target) else {
                    continue;
                };

                let attack_range_m =
                    effective_attack_range_m(proto.attack_range, proto.battle_type);
                let follow_distance_m = mob_follow_distance_m(attack_range_m);
                let attack_threshold_m = mob_attack_threshold_m(attack_range_m);
                let movement_in_flight = mob_movement_in_flight(world, mob_entity, now_ms);
                let target_distance_m = distance(current_pos, target_pos);
                if now_ms >= brain.next_attack_at_ms
                    && target_distance_m <= attack_threshold_m
                    && !movement_in_flight
                {
                    let rot = rotation_from_delta(current_pos, target_pos, current_rot);
                    let timing = attack_timing_for_mob(proto.attack_speed);
                    issue_mob_action(
                        world,
                        map_entity,
                        mob_entity,
                        mob_net_id,
                        MovementKind::Attack,
                        current_pos,
                        current_pos,
                        rot,
                        mob_ref.mob_id,
                        proto.move_speed,
                        &shared,
                        now_ms,
                        now_ts,
                        Some(timing.packet_duration_ms),
                    );
                    if let Some(mut brain_mut) =
                        world.entity_mut(mob_entity).get_mut::<MobBrainState>()
                    {
                        brain_mut.mode = MobBrainMode::Attacking;
                        brain_mut.next_attack_at_ms = now_ms.saturating_add(timing.cooldown_ms);
                        brain_mut.attack_windup_until_ms =
                            now_ms.saturating_add(u64::from(timing.packet_duration_ms));
                        brain_mut.next_chase_rethink_at_ms = brain_mut.attack_windup_until_ms;
                    }
                    continue;
                }

                if now_ms < brain.next_chase_rethink_at_ms {
                    continue;
                }

                let step_len = (target_distance_m - follow_distance_m).max(0.0);
                if step_len <= CLOSE_CHASE_EPSILON_M {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                    continue;
                }

                let desired_goal = clamp_step_towards(current_pos, target_pos, step_len);
                let Some(dst) =
                    next_terrain_valid_destination(navigator.as_ref(), current_pos, desired_goal)
                else {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                    continue;
                };
                if desired_destination_unchanged(world, mob_entity, dst) {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                    continue;
                }
                let rot = rotation_from_delta(current_pos, target_pos, current_rot);
                if issue_mob_action(
                    world,
                    map_entity,
                    mob_entity,
                    mob_net_id,
                    MovementKind::Wait,
                    current_pos,
                    dst,
                    rot,
                    mob_ref.mob_id,
                    proto.move_speed,
                    &shared,
                    now_ms,
                    now_ts,
                    None,
                )
                .is_some()
                {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                }
            }
            MobBrainMode::Returning => {
                if now_ms < brain.next_chase_rethink_at_ms {
                    continue;
                }
                let home_pos = world
                    .entity(mob_entity)
                    .get::<MobHomeAnchor>()
                    .map(|anchor| anchor.pos)
                    .unwrap_or(current_pos);
                let home_dist = distance(current_pos, home_pos);
                if home_dist <= HOME_ARRIVAL_RADIUS_M {
                    continue;
                }
                let Some(dst) =
                    next_terrain_valid_destination(navigator.as_ref(), current_pos, home_pos)
                else {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                    continue;
                };
                if desired_destination_unchanged(world, mob_entity, dst) {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                    continue;
                }
                let rot = rotation_from_delta(current_pos, home_pos, current_rot);
                if issue_mob_action(
                    world,
                    map_entity,
                    mob_entity,
                    mob_net_id,
                    MovementKind::Wait,
                    current_pos,
                    dst,
                    rot,
                    mob_ref.mob_id,
                    proto.move_speed,
                    &shared,
                    now_ms,
                    now_ts,
                    None,
                )
                .is_some()
                {
                    schedule_next_chase_rethink(world, mob_entity, now_ms);
                }
            }
            MobBrainMode::Idle | MobBrainMode::Attacking => {}
        }
    }
}

pub(super) fn mob_follow_distance_m(attack_range_m: f32) -> f32 {
    attack_range_m.max(0.0) * LEGACY_CHASE_FOLLOW_DISTANCE_RATIO
}

pub(super) fn mob_attack_threshold_m(attack_range_m: f32) -> f32 {
    attack_range_m.max(0.0) * LEGACY_ATTACK_THRESHOLD_RATIO
}

fn desired_destination_unchanged(world: &World, mob_entity: Entity, desired: LocalPos) -> bool {
    world
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| distance(motion.0.segment_end_pos, desired) <= DESTINATION_EPSILON_M)
        .unwrap_or(false)
}

fn next_terrain_valid_destination(
    navigator: Option<&Arc<MapNavigator>>,
    current: LocalPos,
    desired_goal: LocalPos,
) -> Option<LocalPos> {
    let Some(navigator) = navigator else {
        return Some(desired_goal);
    };

    if navigator.segment_clear(current, desired_goal) {
        return Some(desired_goal);
    }

    let routed_goal = clamp_step_towards(
        current,
        desired_goal,
        distance(current, desired_goal).min(ROUTED_PLAN_HORIZON_M),
    );

    if navigator.segment_clear(current, routed_goal) {
        return Some(routed_goal);
    }

    navigator
        .next_waypoint(current, routed_goal)
        .filter(|waypoint| distance(current, *waypoint) > CLOSE_CHASE_EPSILON_M)
}

fn schedule_next_chase_rethink(world: &mut World, mob_entity: Entity, now_ms: u64) {
    if let Some(mut brain) = world.entity_mut(mob_entity).get_mut::<MobBrainState>() {
        brain.next_chase_rethink_at_ms = now_ms.saturating_add(LEGACY_CHASE_RETHINK_MS);
    }
}

fn effective_attack_range_m(range: u16, battle_type: MobBattleType) -> f32 {
    let fallback = match battle_type {
        MobBattleType::Melee
        | MobBattleType::Power
        | MobBattleType::Tanker
        | MobBattleType::SuperPower
        | MobBattleType::SuperTanker
        | MobBattleType::Range
        | MobBattleType::Magic
        | MobBattleType::Special => range as f32 / 100.0,
    };
    fallback.max(1.5)
}

fn attack_timing_for_mob(attack_speed: u8) -> AttackTiming {
    let speed = u32::from(attack_speed.max(1));
    let cooldown_ms = (120_000 / speed).clamp(400, 2_000);
    let packet_duration_ms = (cooldown_ms / 2).clamp(200, 1_000);
    AttackTiming {
        cooldown_ms: u64::from(cooldown_ms),
        packet_duration_ms,
    }
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

fn distance(from: LocalPos, to: LocalPos) -> f32 {
    (to - from).length()
}
