use super::state::{
    DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC, MAX_MOVE_PACKET_STEP_M, MobMotionState,
    PlayerMotionState, RuntimeState,
};
use crate::motion::{
    EntityMotionSpeedTable, MotionEntityKey, MotionMoveMode, PlayerMotionProfileKey,
};
use bevy::prelude::*;
use rand::rngs::SmallRng;
use rand::{Rng, RngExt};
use std::time::Duration;
use zohar_domain::Empire;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::mob::spawn::{SpawnTemplate, WeightedGroupChoice};
use zohar_domain::entity::mob::{MobBattleType, MobId};
use zohar_domain::entity::{EntityId, MovementKind};
#[cfg(test)]
use zohar_map_port::MovementArg;
use zohar_map_port::{ClientTimestamp, Facing72, PacketDuration};

use super::rules::{combat, movement};
use super::time::{SimDuration, SimInstant};

pub(crate) use super::resources::next_entity_id;

pub(crate) fn movement_kind_priority(kind: MovementKind) -> u8 {
    match kind {
        MovementKind::Move => 0,
        MovementKind::Wait => 1,
        _ => 2,
    }
}

pub(crate) fn expand_spawn_template(template: &SpawnTemplate, rng: &mut SmallRng) -> Vec<MobId> {
    match template {
        SpawnTemplate::Mob(mob_id) => vec![*mob_id],
        SpawnTemplate::Group(members) => members.iter().copied().collect(),
        SpawnTemplate::GroupGroup(choices) => pick_group_group_choice(choices, rng)
            .map(|choice| choice.members.iter().copied().collect())
            .unwrap_or_default(),
    }
}

pub(crate) fn pick_group_group_choice<'a>(
    choices: &'a [WeightedGroupChoice],
    rng: &mut SmallRng,
) -> Option<&'a WeightedGroupChoice> {
    let total_weight: u32 = choices.iter().map(|choice| choice.weight.max(1)).sum();
    if total_weight == 0 {
        return None;
    }

    let mut ticket = rng.random_range(1..=total_weight);
    for choice in choices {
        let weight = choice.weight.max(1);
        if ticket <= weight {
            return Some(choice);
        }
        ticket -= weight;
    }

    choices.last()
}

pub(crate) fn sanitize_packet_target(current_pos: LocalPos, requested_pos: LocalPos) -> LocalPos {
    clamp_step_towards(current_pos, requested_pos, MAX_MOVE_PACKET_STEP_M)
}

#[cfg(test)]
pub(crate) fn sample_player_motion_at(
    current_pos: LocalPos,
    motion: &mut PlayerMotionState,
    packet_ts: impl Into<ClientTimestamp>,
) -> LocalPos {
    sample_player_motion_state_at(current_pos, *motion, packet_ts.into())
}

pub(crate) fn sample_player_motion_state_at(
    current_pos: LocalPos,
    motion: PlayerMotionState,
    packet_ts: ClientTimestamp,
) -> LocalPos {
    if motion.last_client_ts == ClientTimestamp::ZERO || packet_ts <= motion.last_client_ts {
        return current_pos;
    }
    if motion.segment_end_ts <= motion.segment_start_ts {
        return current_pos;
    }
    if packet_ts >= motion.segment_end_ts {
        return motion.segment_end_pos;
    }

    let total = motion
        .segment_end_ts
        .saturating_sub(motion.segment_start_ts);
    if total == PacketDuration::ZERO {
        return current_pos;
    }
    let elapsed = packet_ts.saturating_sub(motion.segment_start_ts);
    let t = elapsed.get() as f32 / total.get() as f32;
    let delta = motion.segment_end_pos - motion.segment_start_pos;

    motion.segment_start_pos + delta * t
}

pub(crate) fn sample_player_visual_position_at(
    motion: PlayerMotionState,
    packet_ts: ClientTimestamp,
) -> LocalPos {
    if motion.segment_end_ts <= motion.segment_start_ts {
        return motion.segment_end_pos;
    }
    if packet_ts <= motion.segment_start_ts {
        return motion.segment_start_pos;
    }
    if packet_ts >= motion.segment_end_ts {
        return motion.segment_end_pos;
    }

    let total = motion
        .segment_end_ts
        .saturating_sub(motion.segment_start_ts);
    if total == PacketDuration::ZERO {
        return motion.segment_end_pos;
    }
    let elapsed = packet_ts.saturating_sub(motion.segment_start_ts);
    let t = elapsed.get() as f32 / total.get() as f32;
    let delta = motion.segment_end_pos - motion.segment_start_pos;

    motion.segment_start_pos + delta * t
}

pub(crate) fn sample_mob_motion_at(motion: &MobMotionState, now: SimInstant) -> LocalPos {
    if motion.segment_end_at <= motion.segment_start_at {
        return motion.segment_end_pos;
    }
    if now <= motion.segment_start_at {
        return motion.segment_start_pos;
    }
    if now >= motion.segment_end_at {
        return motion.segment_end_pos;
    }

    let total = motion
        .segment_end_at
        .saturating_sub(motion.segment_start_at);
    if total == SimDuration::ZERO {
        return motion.segment_end_pos;
    }
    let elapsed = now.saturating_sub(motion.segment_start_at);
    let t = elapsed.as_millis() as f32 / total.as_millis() as f32;
    let delta = motion.segment_end_pos - motion.segment_start_pos;

    motion.segment_start_pos + delta * t
}

pub(crate) fn clamp_step_towards(from: LocalPos, to: LocalPos, max_step: f32) -> LocalPos {
    let delta = to - from;
    let distance = delta.length();

    if distance <= max_step || distance <= 0.01 {
        return to;
    }

    from + delta * (max_step / distance)
}

pub(crate) fn calculate_move_duration_ms(
    motion_speeds: &EntityMotionSpeedTable,
    appearance: &PlayerAppearance,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> PacketDuration {
    let profile_key = PlayerMotionProfileKey {
        class: appearance.class,
        gender: appearance.gender,
    };
    let motion_speed = motion_speeds
        .speed_for(MotionEntityKey::Player(profile_key), MotionMoveMode::Run)
        .unwrap_or(DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC);
    duration_from_motion_speed(motion_speed, appearance.move_speed, start_pos, target_pos)
}

pub(crate) fn calculate_mob_move_duration_ms(
    motion_speeds: &EntityMotionSpeedTable,
    mob_id: MobId,
    move_mode: MotionMoveMode,
    move_speed_attr: u8,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> PacketDuration {
    let motion_speed = motion_speeds
        .speed_for(MotionEntityKey::Mob(mob_id), move_mode)
        .unwrap_or(DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC);
    duration_from_motion_speed(motion_speed, move_speed_attr, start_pos, target_pos)
}

pub(crate) fn duration_from_motion_speed(
    motion_speed_mps: f32,
    move_speed_attr: u8,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> PacketDuration {
    if motion_speed_mps <= 0.0 || move_speed_attr == 0 {
        return PacketDuration::ZERO;
    }

    let dist = (target_pos - start_pos).length();
    let base_dur = (dist / motion_speed_mps) * 1000.0;
    PacketDuration::new((base_dur * (100.0 / move_speed_attr as f32)) as u32)
}

pub(crate) fn random_protocol_rot(rng: &mut SmallRng) -> Facing72 {
    Facing72::from_wrapped(rng.random_range(0..72))
}

pub(crate) fn random_duration_between_ms(
    rng: &mut SmallRng,
    min: Duration,
    max: Duration,
) -> SimDuration {
    let min_ms = min.as_millis().min(u64::MAX as u128) as u64;
    let max_ms = max.as_millis().min(u64::MAX as u128) as u64;
    let lo = min_ms.min(max_ms);
    let hi = min_ms.max(max_ms);
    SimDuration::from_millis(rng.random_range(lo..=hi))
}

pub(crate) fn rotation_from_delta(
    from: LocalPos,
    to: LocalPos,
    fallback_rot: Facing72,
) -> Facing72 {
    let delta = to - from;
    if delta.square_length() <= 0.0001 {
        return fallback_rot;
    }
    // Match the legacy server's facing convention from `GetDegreeFromPosition`:
    // 0=north, 90=east, 180=south, 270=west.
    let angle = delta.x.atan2(delta.y).to_degrees().rem_euclid(360.0);
    degrees_to_protocol_rot(angle)
}

pub(crate) fn degrees_to_protocol_rot(degrees: f32) -> Facing72 {
    let normalized = degrees.rem_euclid(360.0);
    Facing72::from_wrapped(((normalized / 5.0) as i32).rem_euclid(72) as u8)
}

pub(crate) fn format_global_shout(from_player_name: &str, message_bytes: &[u8]) -> String {
    let text_len = message_bytes
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(message_bytes.len());
    let message_text = String::from_utf8_lossy(&message_bytes[..text_len]);
    format!("{from_player_name} : {message_text}\0")
}

pub(crate) fn format_talking_message(from_name: &str, message_bytes: &[u8]) -> Vec<u8> {
    let text_len = message_bytes
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(message_bytes.len());
    let mut out = Vec::with_capacity(from_name.len() + 3 + text_len + 1);
    out.extend_from_slice(from_name.as_bytes());
    out.extend_from_slice(b" : ");
    out.extend_from_slice(&message_bytes[..text_len]);
    out.push(0);
    out
}

pub(crate) fn resolve_cross_empire_preserve_pct() -> u8 {
    // TODO: replace with recipient-aware policy when language skill, rings, and GM bypass exist.
    10
}

pub(crate) fn obfuscate_cross_empire_talking_body<R: Rng + ?Sized>(
    rng: &mut R,
    source_empire: Empire,
    body_bytes: &mut [u8],
    preserve_pct: u8,
) {
    let preserve_pct = preserve_pct.min(100);
    let mut idx = 0;
    while idx < body_bytes.len() {
        let byte = body_bytes[idx];

        if byte.is_ascii_alphabetic() {
            if should_convert(rng, preserve_pct) {
                body_bytes[idx] = remap_ascii_letter_by_empire(source_empire, byte);
            }
            idx += 1;
            continue;
        }

        if byte & 0x80 != 0 {
            let is_pair = idx + 1 < body_bytes.len() && (body_bytes[idx + 1] & 0x80 != 0);
            if should_convert(rng, preserve_pct) {
                body_bytes[idx] = b'?';
                if is_pair {
                    body_bytes[idx + 1] = b'?';
                }
            }
            idx += if is_pair { 2 } else { 1 };
            continue;
        }

        idx += 1;
    }
}

fn should_convert<R: Rng + ?Sized>(rng: &mut R, preserve_pct: u8) -> bool {
    rng.random_range(1..=100) > preserve_pct
}

fn remap_ascii_letter_by_empire(source_empire: Empire, letter: u8) -> u8 {
    let shift = match source_empire {
        Empire::Red => 5,
        Empire::Yellow => 11,
        Empire::Blue => 17,
    };
    let base = if letter.is_ascii_lowercase() {
        b'a'
    } else {
        b'A'
    };
    ((letter - base + shift) % 26) + base
}

pub(crate) fn validate_player_attack(
    world: &World,
    map_entity: Entity,
    attacker_net_id: EntityId,
    attacker_pos: LocalPos,
    target: EntityId,
) -> Option<Entity> {
    let target_entity = net_entity(world, target)?;
    if target_entity == net_entity(world, attacker_net_id).unwrap_or(Entity::PLACEHOLDER) {
        return None;
    }

    let target_visible = world
        .entity(map_entity)
        .get::<super::state::MapReplication>()
        .is_some_and(|replication| replication.0.is_visible(attacker_net_id, target));
    if !target_visible {
        return None;
    }

    let target_pos = world
        .entity(target_entity)
        .get::<super::state::LocalTransform>()
        .map(|transform| transform.pos)?;

    let max_distance_m =
        combat::PLAYER_MELEE_REACH_M + target_combat_extent_m(world, target_entity);
    (movement::distance(attacker_pos, target_pos) <= max_distance_m).then_some(target_entity)
}

pub(crate) fn net_entity(world: &World, entity_id: EntityId) -> Option<Entity> {
    world
        .resource::<super::state::NetEntityIndex>()
        .0
        .get(&entity_id)
        .copied()
}

pub(crate) fn entity_position(world: &World, entity_id: EntityId) -> Option<LocalPos> {
    let entity = net_entity(world, entity_id)?;
    world
        .entity(entity)
        .get::<super::state::LocalTransform>()
        .map(|transform| transform.pos)
}

pub(crate) fn mob_pack_members(world: &mut World, attacked_mob_entity: Entity) -> Vec<Entity> {
    let attacked_pack_id = world
        .entity(attacked_mob_entity)
        .get::<super::state::MobPackId>()
        .copied()
        .map(|pack| pack.pack_id);

    if let Some(pack_id) = attacked_pack_id {
        let mut query = world.query::<(Entity, &super::state::MobPackId)>();
        query
            .iter(world)
            .filter_map(|(entity, mob_pack_id)| (mob_pack_id.pack_id == pack_id).then_some(entity))
            .collect()
    } else {
        vec![attacked_mob_entity]
    }
}

pub(crate) fn acquire_aggressive_target(
    world: &World,
    map_entity: Entity,
    current_pos: LocalPos,
    proto: &zohar_domain::entity::mob::MobPrototype,
) -> Option<EntityId> {
    if !proto
        .bhv_flags
        .contains(zohar_domain::BehaviorFlags::AGGRESSIVE)
    {
        return None;
    }
    let sight_m = proto.aggressive_sight as f32 / 100.0;
    if sight_m <= 0.0 {
        return None;
    }
    let spatial = world.entity(map_entity).get::<super::state::MapSpatial>()?;
    spatial
        .0
        .query_in_radius(current_pos, sight_m)
        .filter_map(|candidate| {
            let entity = net_entity(world, candidate)?;
            world.entity(entity).get::<super::state::PlayerMarker>()?;
            let pos = world
                .entity(entity)
                .get::<super::state::LocalTransform>()?
                .pos;
            Some((candidate, movement::distance(current_pos, pos)))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(candidate, _)| candidate)
}

pub(crate) fn player_position(
    world: &World,
    entity_id: EntityId,
    packet_ts: ClientTimestamp,
) -> Option<LocalPos> {
    let entity = net_entity(world, entity_id)?;
    world.entity(entity).get::<super::state::PlayerMarker>()?;
    let transform = world.entity(entity).get::<super::state::LocalTransform>()?;
    let motion = world
        .entity(entity)
        .get::<super::state::PlayerMotion>()
        .map(|motion| motion.0)
        .unwrap_or(PlayerMotionState {
            segment_start_pos: transform.pos,
            segment_end_pos: transform.pos,
            segment_start_ts: packet_ts,
            segment_end_ts: packet_ts,
            last_client_ts: ClientTimestamp::ZERO,
        });
    Some(sample_player_visual_position_at(motion, packet_ts))
}

pub(crate) fn chase_target_position(
    world: &World,
    current_pos: LocalPos,
    entity_id: EntityId,
    packet_ts: ClientTimestamp,
    mob_id: MobId,
    mob_move_speed: u8,
    battle_type: MobBattleType,
    shared: &super::state::SharedConfig,
) -> Option<LocalPos> {
    let target_pos = player_position(world, entity_id, packet_ts)?;
    let entity = net_entity(world, entity_id)?;
    let motion = world
        .entity(entity)
        .get::<super::state::PlayerMotion>()
        .map(|motion| motion.0)?;
    if matches!(battle_type, MobBattleType::Range | MobBattleType::Magic) {
        return Some(target_pos);
    }
    Some(
        predict_moving_target_position(
            current_pos,
            target_pos,
            motion,
            packet_ts,
            mob_id,
            mob_move_speed,
            shared,
        )
        .unwrap_or(target_pos),
    )
}

pub(crate) fn has_exceeded_leash(
    current_pos: LocalPos,
    target_pos: LocalPos,
    home_pos: LocalPos,
) -> bool {
    const MOB_LEASH_DISTANCE_M: Option<f32> = None;

    let Some(leash_distance_m) = MOB_LEASH_DISTANCE_M else {
        return false;
    };
    movement::distance(current_pos, home_pos) > leash_distance_m
        || movement::distance(target_pos, home_pos) > leash_distance_m
}

pub(crate) fn sample_mob_motion(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let now = world.resource::<RuntimeState>().sim_now;

    let updates = {
        let mut query = world.query::<(
            Entity,
            &super::state::NetEntityId,
            &super::state::MobMotion,
            &mut super::state::LocalTransform,
        )>();
        query
            .iter_mut(world)
            .filter_map(|(_entity, net_id, motion, transform)| {
                let sampled_pos = sample_mob_motion_at(&motion.0, now);
                (sampled_pos != transform.pos).then_some((net_id.net_id, sampled_pos))
            })
            .collect::<Vec<_>>()
    };
    if updates.is_empty() {
        return;
    }

    let net_index = world.resource::<super::state::NetEntityIndex>().0.clone();
    for (net_id, sampled_pos) in &updates {
        let Some(entity) = net_index.get(net_id).copied() else {
            continue;
        };
        if let Some(mut transform) = world
            .entity_mut(entity)
            .get_mut::<super::state::LocalTransform>()
        {
            transform.pos = *sampled_pos;
        }
    }

    if let Some(mut spatial) = world
        .entity_mut(map_entity)
        .get_mut::<super::state::MapSpatial>()
    {
        for (net_id, sampled_pos) in &updates {
            spatial.0.update_position(*net_id, *sampled_pos);
        }
    }
    world.resource_mut::<RuntimeState>().is_dirty = true;
}

pub(crate) fn sampled_mob_position(
    world: &World,
    mob_entity: Entity,
    now: SimInstant,
) -> Option<LocalPos> {
    let transform = world
        .entity(mob_entity)
        .get::<super::state::LocalTransform>()?;
    let Some(motion) = world
        .entity(mob_entity)
        .get::<super::state::MobMotion>()
        .map(|motion| motion.0)
    else {
        return Some(transform.pos);
    };
    Some(sample_mob_motion_at(&motion, now))
}

pub(crate) fn mob_movement_in_flight(world: &World, mob_entity: Entity, now: SimInstant) -> bool {
    let Some(motion) = world
        .entity(mob_entity)
        .get::<super::state::MobMotion>()
        .map(|motion| motion.0)
    else {
        return false;
    };
    motion.segment_end_at > now && motion.segment_end_pos != motion.segment_start_pos
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn issue_mob_action(
    world: &mut World,
    map_entity: Entity,
    mob_entity: Entity,
    mob_net_id: EntityId,
    kind: MovementKind,
    start_pos: LocalPos,
    target_pos: LocalPos,
    rot: Facing72,
    mob_id: MobId,
    move_speed: u8,
    shared: &super::state::SharedConfig,
    now: impl Into<SimInstant>,
    now_ts: impl Into<ClientTimestamp>,
    duration_override_ms: Option<PacketDuration>,
) -> Option<PacketDuration> {
    let now = now.into();
    let now_ts = now_ts.into();
    let start_pos = snap_local_to_wire_cm(start_pos);
    let target_pos = snap_local_to_wire_cm(target_pos);
    let duration = duration_override_ms.unwrap_or_else(|| {
        calculate_mob_movement_duration(shared, mob_id, move_speed, kind, start_pos, target_pos)
    });
    if matches!(kind, MovementKind::Wait | MovementKind::Move) && duration == PacketDuration::ZERO {
        return None;
    }

    if let Some(mut transform) = world
        .entity_mut(mob_entity)
        .get_mut::<super::state::LocalTransform>()
    {
        transform.pos = start_pos;
        transform.rot = rot;
    }
    if let Some(mut motion) = world
        .entity_mut(mob_entity)
        .get_mut::<super::state::MobMotion>()
    {
        motion.0 = match kind {
            MovementKind::Wait | MovementKind::Move => MobMotionState {
                segment_start_pos: start_pos,
                segment_end_pos: target_pos,
                segment_start_at: now,
                segment_end_at: now.saturating_add(SimDuration::from_packet_duration(duration)),
            },
            _ => MobMotionState {
                segment_start_pos: start_pos,
                segment_end_pos: start_pos,
                segment_start_at: now,
                segment_end_at: now,
            },
        };
    }
    if let Some(mut spatial) = world
        .entity_mut(map_entity)
        .get_mut::<super::state::MapSpatial>()
    {
        spatial.0.update_position(mob_net_id, start_pos);
    }
    if let Some(mut pending) = world
        .entity_mut(map_entity)
        .get_mut::<super::state::MapPendingMovements>()
    {
        pending.0.push(super::state::PendingMovement {
            mover_player_id: None,
            entity_id: mob_net_id,
            new_pos: target_pos,
            kind,
            reliable: kind == MovementKind::Attack,
            arg: MovementArg::ZERO,
            rot,
            ts: now_ts,
            duration,
        });
    }
    world.resource_mut::<RuntimeState>().is_dirty = true;
    Some(duration)
}

pub(crate) fn snap_local_to_wire_cm(pos: LocalPos) -> LocalPos {
    const WIRE_CM_PER_METER: f32 = 100.0;

    fn trunc_to_wire_cm(coord_m: f32) -> f32 {
        (coord_m * WIRE_CM_PER_METER).trunc() / WIRE_CM_PER_METER
    }

    LocalPos::new(trunc_to_wire_cm(pos.x), trunc_to_wire_cm(pos.y))
}

fn target_combat_extent_m(world: &World, target_entity: Entity) -> f32 {
    let Some(mob_ref) = world.entity(target_entity).get::<super::state::MobRef>() else {
        return 0.0;
    };
    let Some(proto) = world
        .resource::<super::state::SharedConfig>()
        .mobs
        .get(&mob_ref.mob_id)
    else {
        return 0.0;
    };
    proto.combat_extent_m.max(0.0)
}

fn predict_moving_target_position(
    current_pos: LocalPos,
    target_pos: LocalPos,
    motion: PlayerMotionState,
    packet_ts: ClientTimestamp,
    mob_id: MobId,
    mob_move_speed: u8,
    shared: &super::state::SharedConfig,
) -> Option<LocalPos> {
    if motion.segment_end_ts <= motion.segment_start_ts || packet_ts >= motion.segment_end_ts {
        return None;
    }

    let remaining_ms = motion.segment_end_ts.saturating_sub(packet_ts);
    if remaining_ms == PacketDuration::ZERO {
        return None;
    }

    let target_delta = motion.segment_end_pos - target_pos;
    if target_delta.length() <= movement::CLOSE_CHASE_EPSILON_M {
        return None;
    }

    let to_target = target_pos - current_pos;
    let current_distance = to_target.length();
    if current_distance <= movement::CLOSE_CHASE_EPSILON_M {
        return None;
    }

    let remaining_s = remaining_ms.get() as f32 / 1000.0;
    let target_velocity = target_delta / remaining_s;
    let mob_speed_mps = movement::mob_run_speed_mps(shared, mob_id, mob_move_speed)?;

    let closing_speed = mob_speed_mps - target_velocity.dot(to_target / current_distance);
    if closing_speed < 0.1 {
        return None;
    }

    let meet_time_s = current_distance / closing_speed;
    if !meet_time_s.is_finite() || meet_time_s <= 0.0 {
        return None;
    }

    let predicted_target = target_pos + target_velocity * meet_time_s;
    Some(
        if movement::distance(current_pos, predicted_target) > current_distance {
            clamp_step_towards(current_pos, predicted_target, current_distance)
        } else {
            predicted_target
        },
    )
}

#[cfg(test)]
fn calculate_mob_movement_duration(
    shared: &super::state::SharedConfig,
    mob_id: MobId,
    move_speed: u8,
    kind: MovementKind,
    from: LocalPos,
    to: LocalPos,
) -> PacketDuration {
    if kind != MovementKind::Move && kind != MovementKind::Wait {
        return PacketDuration::ZERO;
    }
    calculate_mob_move_duration_ms(
        &shared.motion_speeds,
        mob_id,
        MotionMoveMode::Run,
        move_speed,
        from,
        to,
    )
}
