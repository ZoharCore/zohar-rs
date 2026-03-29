use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::player::PlayerId;

use super::super::mob_motion::{sampled_mob_position, snap_local_to_wire_cm};
use super::super::query;
use super::super::rules::movement;
use super::super::state::{
    LocalTransform, NetEntityId, PlayerAppearanceComp, PlayerMotion, PlayerMotionState,
    RuntimeState, SharedConfig,
};
use super::super::util::{
    calculate_move_duration_ms, packet_time_ms, rotation_from_delta, sample_player_motion_state_at,
    sanitize_packet_target,
};
use super::{Action, MobActionCompletion};

pub(crate) fn build_player_move_action(
    world: &World,
    shared: &SharedConfig,
    map_size: zohar_domain::coords::LocalSize,
    player_entity: Entity,
    player_id: PlayerId,
    kind: MovementKind,
    arg: u8,
    rot: u8,
    target: LocalPos,
    ts: u32,
) -> Option<Action> {
    let entity_id = world
        .entity(player_entity)
        .get::<NetEntityId>()
        .map(|net_id| net_id.net_id)?;
    let transform = world.entity(player_entity).get::<LocalTransform>()?;
    let player_motion = world.entity(player_entity).get::<PlayerMotion>()?;
    let appearance = world
        .entity(player_entity)
        .get::<PlayerAppearanceComp>()
        .map(|appearance| appearance.0.clone())?;

    let old_pos = sample_player_motion_state_at(transform.pos, player_motion.0, ts);
    let requested_pos = sanitize_packet_target(old_pos, target);
    let end_pos = movement::sanitize_player_target_to_map(requested_pos, map_size);
    let duration = if kind == MovementKind::Move {
        calculate_move_duration_ms(&shared.motion_speeds, &appearance, old_pos, end_pos)
    } else {
        0
    };
    let motion = if kind == MovementKind::Move && duration > 0 {
        PlayerMotionState {
            segment_start_pos: old_pos,
            segment_end_pos: end_pos,
            segment_start_ts: ts,
            segment_end_ts: ts.saturating_add(duration),
            last_client_ts: ts,
        }
    } else {
        PlayerMotionState {
            segment_start_pos: end_pos,
            segment_end_pos: end_pos,
            segment_start_ts: ts,
            segment_end_ts: ts,
            last_client_ts: ts,
        }
    };

    Some(Action::PlayerMotion {
        player_entity,
        player_id,
        entity_id,
        kind,
        arg,
        rot,
        end_pos,
        ts,
        duration,
        motion,
    })
}

pub(crate) fn build_player_attack_action(
    world: &World,
    player_entity: Entity,
    target: zohar_domain::entity::EntityId,
    attack_type: u8,
) -> Option<Action> {
    let now_ts = packet_time_ms(world.resource::<RuntimeState>().packet_time_start);
    let entity_id = world
        .entity(player_entity)
        .get::<NetEntityId>()
        .map(|net_id| net_id.net_id)?;
    let transform = world.entity(player_entity).get::<LocalTransform>()?;
    let rot = query::entity_position(world, target)
        .map(|target_pos| rotation_from_delta(transform.pos, target_pos, transform.rot))
        .unwrap_or(transform.rot);

    Some(Action::PlayerAttack {
        player_entity,
        entity_id,
        pos: transform.pos,
        rot,
        attack_type,
        ts: now_ts,
        duration: 600,
    })
}

pub(crate) fn build_mob_move_action(
    world: &World,
    mob_entity: Entity,
    kind: MovementKind,
    destination: LocalPos,
    face_to: LocalPos,
    next_brain: super::super::state::MobBrainState,
    completion: MobActionCompletion,
) -> Option<Action> {
    let now_ms = world
        .resource::<super::super::state::RuntimeState>()
        .sim_time_ms;
    let now_ts = packet_time_ms(
        world
            .resource::<super::super::state::RuntimeState>()
            .packet_time_start,
    );
    let shared = world.resource::<SharedConfig>();
    let entity_id = world.entity(mob_entity).get::<NetEntityId>()?.net_id;
    let mob_ref = world
        .entity(mob_entity)
        .get::<super::super::state::MobRef>()?;
    let current_rot = world.entity(mob_entity).get::<LocalTransform>()?.rot;
    let start_pos = sampled_mob_position(world, mob_entity, now_ms)?;
    let rot = rotation_from_delta(start_pos, face_to, current_rot);
    let duration = movement::calculate_mob_duration(
        shared,
        mob_ref.mob_id,
        kind,
        start_pos,
        destination,
        shared
            .mobs
            .get(&mob_ref.mob_id)
            .map(|proto| proto.move_speed)
            .unwrap_or(0),
    )?;

    Some(Action::MobMotion {
        mob_entity,
        entity_id,
        start_pos: snap_local_to_wire_cm(start_pos),
        end_pos: snap_local_to_wire_cm(destination),
        rot,
        kind,
        ts: now_ts,
        duration,
        next_brain: resolve_mob_follow_up(next_brain, completion, now_ms, duration),
    })
}

pub(crate) fn build_mob_attack_action(
    world: &World,
    mob_entity: Entity,
    face_to: LocalPos,
    windup_duration_ms: u32,
    next_brain: super::super::state::MobBrainState,
    completion: MobActionCompletion,
) -> Option<Action> {
    let now_ms = world
        .resource::<super::super::state::RuntimeState>()
        .sim_time_ms;
    let now_ts = packet_time_ms(
        world
            .resource::<super::super::state::RuntimeState>()
            .packet_time_start,
    );
    let entity_id = world.entity(mob_entity).get::<NetEntityId>()?.net_id;
    let start_pos = sampled_mob_position(world, mob_entity, now_ms)?;
    let current_rot = world.entity(mob_entity).get::<LocalTransform>()?.rot;
    let rot = rotation_from_delta(start_pos, face_to, current_rot);

    Some(Action::MobAttack {
        mob_entity,
        entity_id,
        pos: snap_local_to_wire_cm(start_pos),
        rot,
        ts: now_ts,
        duration: windup_duration_ms,
        next_brain: resolve_mob_follow_up(next_brain, completion, now_ms, windup_duration_ms),
    })
}

fn resolve_mob_follow_up(
    mut next_brain: super::super::state::MobBrainState,
    completion: MobActionCompletion,
    now_ms: u64,
    action_duration_ms: u32,
) -> super::super::state::MobBrainState {
    match completion {
        MobActionCompletion::None => {}
        MobActionCompletion::RethinkAtActionEnd => {
            next_brain.next_rethink_at_ms = now_ms.saturating_add(u64::from(action_duration_ms));
        }
        MobActionCompletion::RethinkAtActionEndOrDelay { max_delay_ms } => {
            next_brain.next_rethink_at_ms =
                now_ms.saturating_add(u64::from(action_duration_ms).min(max_delay_ms));
        }
        MobActionCompletion::IdleWander { post_move_pause_ms } => {
            let movement_end_ms = now_ms.saturating_add(u64::from(action_duration_ms));
            next_brain.wander_wait_until_ms = Some(movement_end_ms);
            next_brain.wander_next_decision_at_ms =
                movement_end_ms.saturating_add(post_move_pause_ms);
        }
    }

    next_brain
}
