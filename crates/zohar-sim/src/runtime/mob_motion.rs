use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::mob::MobId;
use zohar_domain::entity::{EntityId, MovementKind};

use crate::motion::MotionMoveMode;

use super::state::{
    LocalTransform, MapPendingMovements, MapSpatial, MobMotion, MobMotionState, PendingMovement,
    RuntimeState, SharedConfig,
};
use super::util::{calculate_mob_move_duration_ms, sample_mob_motion_at};

const WIRE_CM_PER_METER: f32 = 100.0;

pub(super) fn sample_mob_motion(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let now_ms = world.resource::<RuntimeState>().sim_time_ms;

    let updates = {
        let mut query = world.query::<(
            Entity,
            &super::state::NetEntityId,
            &MobMotion,
            &mut LocalTransform,
        )>();
        query
            .iter_mut(world)
            .filter_map(|(_entity, net_id, motion, transform)| {
                let sampled_pos = sample_mob_motion_at(&motion.0, now_ms);
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
        if let Some(mut transform) = world.entity_mut(entity).get_mut::<LocalTransform>() {
            transform.pos = *sampled_pos;
        }
    }

    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        for (net_id, sampled_pos) in &updates {
            spatial.0.update_position(*net_id, *sampled_pos);
        }
    }
    world.resource_mut::<RuntimeState>().is_dirty = true;
}

pub(super) fn sampled_mob_position(
    world: &World,
    mob_entity: Entity,
    now_ms: u64,
) -> Option<LocalPos> {
    let transform = world.entity(mob_entity).get::<LocalTransform>()?;
    let Some(motion) = world
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0)
    else {
        return Some(transform.pos);
    };
    Some(sample_mob_motion_at(&motion, now_ms))
}

pub(super) fn mob_movement_in_flight(world: &World, mob_entity: Entity, now_ms: u64) -> bool {
    let Some(motion) = world
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0)
    else {
        return false;
    };
    motion.segment_end_at_ms > now_ms && motion.segment_end_pos != motion.segment_start_pos
}

#[allow(clippy::too_many_arguments)]
pub(super) fn issue_mob_action(
    world: &mut World,
    map_entity: Entity,
    mob_entity: Entity,
    mob_net_id: EntityId,
    kind: MovementKind,
    start_pos: LocalPos,
    target_pos: LocalPos,
    rot: u8,
    mob_id: MobId,
    move_speed: u8,
    shared: &SharedConfig,
    now_ms: u64,
    now_ts: u32,
    duration_override_ms: Option<u32>,
) -> Option<u32> {
    let start_pos = snap_local_to_wire_cm(start_pos);
    let target_pos = snap_local_to_wire_cm(target_pos);
    let duration = duration_override_ms.unwrap_or_else(|| {
        calculate_mob_movement_duration(shared, mob_id, move_speed, kind, start_pos, target_pos)
    });
    if matches!(kind, MovementKind::Wait | MovementKind::Move) && duration == 0 {
        return None;
    }

    if let Some(mut transform) = world.entity_mut(mob_entity).get_mut::<LocalTransform>() {
        transform.pos = start_pos;
        transform.rot = rot;
    }
    if let Some(mut motion) = world.entity_mut(mob_entity).get_mut::<MobMotion>() {
        motion.0 = match kind {
            MovementKind::Wait | MovementKind::Move => MobMotionState {
                segment_start_pos: start_pos,
                segment_end_pos: target_pos,
                segment_start_at_ms: now_ms,
                segment_end_at_ms: now_ms.saturating_add(u64::from(duration)),
            },
            _ => MobMotionState {
                segment_start_pos: start_pos,
                segment_end_pos: start_pos,
                segment_start_at_ms: now_ms,
                segment_end_at_ms: now_ms,
            },
        };
    }
    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        spatial.0.update_position(mob_net_id, start_pos);
    }
    if let Some(mut pending) = world
        .entity_mut(map_entity)
        .get_mut::<MapPendingMovements>()
    {
        pending.0.push(PendingMovement {
            mover_player_id: None,
            entity_id: mob_net_id,
            new_pos: target_pos,
            kind,
            reliable: kind == MovementKind::Attack,
            arg: 0,
            rot,
            ts: now_ts,
            duration,
        });
    }
    world.resource_mut::<RuntimeState>().is_dirty = true;
    Some(duration)
}

fn calculate_mob_movement_duration(
    shared: &SharedConfig,
    mob_id: MobId,
    move_speed: u8,
    kind: MovementKind,
    from: LocalPos,
    to: LocalPos,
) -> u32 {
    if kind != MovementKind::Move && kind != MovementKind::Wait {
        return 0;
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

pub(super) fn snap_local_to_wire_cm(pos: LocalPos) -> LocalPos {
    LocalPos::new(trunc_to_wire_cm(pos.x), trunc_to_wire_cm(pos.y))
}

fn trunc_to_wire_cm(coord_m: f32) -> f32 {
    (coord_m * WIRE_CM_PER_METER).trunc() / WIRE_CM_PER_METER
}
