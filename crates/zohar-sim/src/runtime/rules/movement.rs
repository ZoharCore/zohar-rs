use zohar_domain::coords::{LocalPos, LocalSize};
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::mob::MobId;
use zohar_map_port::PacketDuration;

use crate::motion::{MotionEntityKey, MotionMoveMode};

use super::super::mob_motion::snap_local_to_wire_cm;
use super::super::state::SharedConfig;
use super::super::util::calculate_mob_move_duration_ms;

pub(crate) const CLOSE_CHASE_EPSILON_M: f32 = 0.01;

pub(crate) fn sanitize_player_target_to_map(requested: LocalPos, map_size: LocalSize) -> LocalPos {
    LocalPos::new(
        clamp_axis_to_map(requested.x, map_size.width),
        clamp_axis_to_map(requested.y, map_size.height),
    )
}

pub(crate) fn calculate_mob_duration(
    shared: &SharedConfig,
    mob_id: MobId,
    kind: MovementKind,
    from: LocalPos,
    to: LocalPos,
    move_speed: u8,
) -> Option<PacketDuration> {
    if kind != MovementKind::Move && kind != MovementKind::Wait {
        return None;
    }
    let duration = calculate_mob_move_duration_ms(
        &shared.motion_speeds,
        mob_id,
        MotionMoveMode::Run,
        move_speed,
        snap_local_to_wire_cm(from),
        snap_local_to_wire_cm(to),
    );
    (duration > PacketDuration::ZERO).then_some(duration)
}

pub(crate) fn desired_destination_unchanged(
    current_segment_end_pos: Option<LocalPos>,
    desired: LocalPos,
) -> bool {
    current_segment_end_pos
        .is_some_and(|segment_end_pos| distance(segment_end_pos, desired) <= 0.05)
}

pub(crate) fn distance(from: LocalPos, to: LocalPos) -> f32 {
    (to - from).length()
}

pub(crate) fn mob_run_speed_mps(
    shared: &SharedConfig,
    mob_id: MobId,
    move_speed: u8,
) -> Option<f32> {
    let speed = shared
        .motion_speeds
        .speed_for(MotionEntityKey::Mob(mob_id), MotionMoveMode::Run)
        .unwrap_or(super::super::state::DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC);
    let scaled = speed * (move_speed as f32 / 100.0);
    (scaled > 0.0).then_some(scaled)
}

fn clamp_axis_to_map(coord: f32, max_exclusive: f32) -> f32 {
    if !coord.is_finite() {
        return 0.0;
    }
    if !max_exclusive.is_finite() || max_exclusive <= 0.0 {
        return 0.0;
    }
    coord.clamp(0.0, max_exclusive - 0.001)
}
