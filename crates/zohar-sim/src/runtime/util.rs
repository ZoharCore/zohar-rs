use crate::motion::{
    EntityMotionSpeedTable, MotionEntityKey, MotionMoveMode, PlayerMotionProfileKey,
};
use rand::rngs::SmallRng;
use rand::{Rng, RngExt};
use std::time::{Duration, Instant};
use zohar_domain::Empire;
use zohar_domain::MobId;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::{EntityId, MovementKind};
use zohar_domain::mob::{SpawnTemplate, WeightedGroupChoice};

use super::state::{
    DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC, MAX_MOVE_PACKET_STEP_M, MonsterWanderConfig,
    PlayerMotionState, RuntimeState,
};

pub(super) fn movement_kind_priority(kind: MovementKind) -> u8 {
    match kind {
        MovementKind::Move => 0,
        MovementKind::Wait => 1,
        _ => 2,
    }
}

pub(super) fn next_entity_id(state: &mut RuntimeState) -> EntityId {
    state.next_net_id = state.next_net_id.wrapping_add(1);
    if state.next_net_id == 0 {
        state.next_net_id = 1;
    }
    EntityId(state.next_net_id)
}

pub(super) fn expand_spawn_template(template: &SpawnTemplate, rng: &mut SmallRng) -> Vec<MobId> {
    match template {
        SpawnTemplate::Mob(mob_id) => vec![*mob_id],
        SpawnTemplate::Group(members) => members.iter().copied().collect(),
        SpawnTemplate::GroupGroup(choices) => pick_group_group_choice(choices, rng)
            .map(|choice| choice.members.iter().copied().collect())
            .unwrap_or_default(),
    }
}

pub(super) fn pick_group_group_choice<'a>(
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

pub(super) fn sanitize_packet_target(current_pos: LocalPos, requested_pos: LocalPos) -> LocalPos {
    clamp_step_towards(current_pos, requested_pos, MAX_MOVE_PACKET_STEP_M)
}

pub(super) fn sample_player_motion_at(
    current_pos: LocalPos,
    motion: &mut PlayerMotionState,
    packet_ts: u32,
) -> LocalPos {
    if motion.last_client_ts == 0 || packet_ts <= motion.last_client_ts {
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
    if total == 0 {
        return current_pos;
    }
    let elapsed = packet_ts.saturating_sub(motion.segment_start_ts);
    let t = elapsed as f32 / total as f32;
    let dx = motion.segment_end_pos.x - motion.segment_start_pos.x;
    let dy = motion.segment_end_pos.y - motion.segment_start_pos.y;
    LocalPos::new(
        motion.segment_start_pos.x + dx * t,
        motion.segment_start_pos.y + dy * t,
    )
}

pub(super) fn clamp_step_towards(from: LocalPos, to: LocalPos, max_step: f32) -> LocalPos {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let distance = (dx * dx + dy * dy).sqrt();

    if distance <= max_step || distance <= 0.01 {
        return to;
    }

    let ratio = max_step / distance;
    LocalPos::new(from.x + dx * ratio, from.y + dy * ratio)
}

pub(super) fn calculate_move_duration_ms(
    motion_speeds: &EntityMotionSpeedTable,
    appearance: &PlayerAppearance,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> u32 {
    let profile_key = PlayerMotionProfileKey {
        class: appearance.class,
        gender: appearance.gender,
    };
    let motion_speed = motion_speeds
        .speed_for(MotionEntityKey::Player(profile_key), MotionMoveMode::Run)
        .unwrap_or(DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC);
    duration_from_motion_speed(motion_speed, appearance.move_speed, start_pos, target_pos)
}

pub(super) fn calculate_mob_move_duration_ms(
    motion_speeds: &EntityMotionSpeedTable,
    mob_id: MobId,
    move_speed_attr: u8,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> u32 {
    let motion_speed = motion_speeds
        .speed_for(MotionEntityKey::Mob(mob_id), MotionMoveMode::Run)
        .unwrap_or(DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC);
    duration_from_motion_speed(motion_speed, move_speed_attr, start_pos, target_pos)
}

pub(super) fn duration_from_motion_speed(
    motion_speed_mps: f32,
    move_speed_attr: u8,
    start_pos: LocalPos,
    target_pos: LocalPos,
) -> u32 {
    let dx = target_pos.x - start_pos.x;
    let dy = target_pos.y - start_pos.y;
    let dist = (dx * dx + dy * dy).sqrt();
    if motion_speed_mps <= 0.0 {
        return 0;
    }

    let base_dur = (dist / motion_speed_mps) * 1000.0;
    let i = 100 - move_speed_attr as i32;
    let scale = if i > 0 {
        100 + i
    } else if i < 0 {
        10000 / (100 - i)
    } else {
        100
    };

    ((base_dur * scale as f32) / 100.0) as u32
}

pub(super) fn random_protocol_rot(rng: &mut SmallRng) -> u8 {
    rng.random_range(0..72)
}

pub(super) fn random_idle_decision_delay(rng: &mut SmallRng, cfg: &MonsterWanderConfig) -> u64 {
    random_duration_between_ms(
        rng,
        cfg.decision_pause_idle_min,
        cfg.decision_pause_idle_max,
    )
}

pub(super) fn random_post_move_delay(rng: &mut SmallRng, cfg: &MonsterWanderConfig) -> u64 {
    random_duration_between_ms(rng, cfg.post_move_pause_min, cfg.post_move_pause_max)
}

pub(super) fn random_duration_between_ms(rng: &mut SmallRng, min: Duration, max: Duration) -> u64 {
    let min_ms = min.as_millis().min(u64::MAX as u128) as u64;
    let max_ms = max.as_millis().min(u64::MAX as u128) as u64;
    let lo = min_ms.min(max_ms);
    let hi = min_ms.max(max_ms);
    rng.random_range(lo..=hi)
}

pub(super) fn rotation_from_delta(from: LocalPos, to: LocalPos, fallback_rot: u8) -> u8 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if (dx * dx + dy * dy) <= 0.0001 {
        return fallback_rot;
    }
    let angle = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    degrees_to_protocol_rot(angle)
}

pub(super) fn degrees_to_protocol_rot(degrees: f32) -> u8 {
    let normalized = degrees.rem_euclid(360.0);
    ((normalized / 5.0) as i32).rem_euclid(72) as u8
}

pub(super) fn packet_time_ms(packet_time_start: Instant) -> u32 {
    packet_time_start
        .elapsed()
        .as_millis()
        .min(u32::MAX as u128) as u32
}

pub(super) fn format_global_shout(from_player_name: &str, message_bytes: &[u8]) -> String {
    let text_len = message_bytes
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(message_bytes.len());
    let message_text = String::from_utf8_lossy(&message_bytes[..text_len]);
    format!("{from_player_name} : {message_text}\0")
}

pub(super) fn format_talking_message(from_name: &str, message_bytes: &[u8]) -> Vec<u8> {
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

pub(super) fn resolve_cross_empire_preserve_pct() -> u8 {
    // TODO: replace with recipient-aware policy when language skill, rings, and GM bypass exist.
    10
}

pub(super) fn obfuscate_cross_empire_talking_body<R: Rng + ?Sized>(
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
