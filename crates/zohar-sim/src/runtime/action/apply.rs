use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::player::PlayerId;
use zohar_map_port::{AttackIntent, ClientTimestamp, Facing72, MovementArg, PacketDuration};

use super::super::state::{MapPendingMovements, RuntimeState};
use super::{Action, ActionBuffer};

pub(crate) fn process_actions(world: &mut World) {
    let actions = {
        let mut buffer = world.resource_mut::<ActionBuffer>();
        std::mem::take(&mut buffer.0)
    };
    if actions.is_empty() {
        return;
    }

    for action in actions {
        apply_action(world, action);
    }
}

pub(crate) fn apply_action(world: &mut World, action: Action) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let now = world.resource::<RuntimeState>().sim_now;

    let (movement, dirty) = match action {
        Action::PlayerMotion {
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
        } => (
            apply_player_motion(
                world,
                map_entity,
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
            ),
            true,
        ),
        Action::PlayerAttack {
            player_entity,
            entity_id,
            pos,
            rot,
            attack,
            ts,
            duration,
        } => (
            Some(apply_player_attack(
                world,
                player_entity,
                entity_id,
                pos,
                rot,
                attack,
                ts,
                duration,
            )),
            false,
        ),
        Action::MobMotion {
            mob_entity,
            entity_id,
            start_pos,
            end_pos,
            rot,
            kind,
            ts,
            duration,
            next_brain,
        } => (
            Some(apply_mob_motion(
                world, map_entity, now, mob_entity, entity_id, start_pos, end_pos, rot, kind, ts,
                duration, next_brain,
            )),
            true,
        ),
        Action::MobAttack {
            mob_entity,
            entity_id,
            pos,
            rot,
            ts,
            duration,
            next_brain,
        } => (
            Some(apply_mob_attack(
                world, map_entity, now, mob_entity, entity_id, pos, rot, ts, duration, next_brain,
            )),
            true,
        ),
    };

    if dirty {
        world.resource_mut::<RuntimeState>().is_dirty = true;
    }
    if let Some(movement) = movement {
        push_pending_movement(world, map_entity, movement);
    }
}

fn apply_player_motion(
    world: &mut World,
    map_entity: Entity,
    player_entity: Entity,
    player_id: PlayerId,
    entity_id: zohar_domain::entity::EntityId,
    kind: MovementKind,
    arg: MovementArg,
    rot: Facing72,
    end_pos: LocalPos,
    ts: ClientTimestamp,
    duration: PacketDuration,
    motion: super::super::state::PlayerMotionState,
) -> Option<super::super::state::PendingMovement> {
    if let Some(mut transform) = world
        .entity_mut(player_entity)
        .get_mut::<super::super::state::LocalTransform>()
    {
        transform.pos = end_pos;
        transform.rot = rot;
    }
    if let Some(mut player_motion) = world
        .entity_mut(player_entity)
        .get_mut::<super::super::state::PlayerMotion>()
    {
        player_motion.0 = motion;
    }
    if let Some(mut spatial) = world
        .entity_mut(map_entity)
        .get_mut::<super::super::state::MapSpatial>()
    {
        spatial.0.update_position(entity_id, end_pos);
    }

    Some(super::super::state::PendingMovement {
        mover_player_id: Some(player_id),
        entity_id,
        new_pos: end_pos,
        kind,
        reliable: false,
        arg,
        rot,
        ts,
        duration,
    })
}

fn apply_player_attack(
    world: &mut World,
    player_entity: Entity,
    entity_id: zohar_domain::entity::EntityId,
    pos: LocalPos,
    rot: Facing72,
    _attack: AttackIntent,
    ts: ClientTimestamp,
    duration: PacketDuration,
) -> super::super::state::PendingMovement {
    if let Some(mut transform) = world
        .entity_mut(player_entity)
        .get_mut::<super::super::state::LocalTransform>()
    {
        transform.rot = rot;
    }

    super::super::state::PendingMovement {
        mover_player_id: Some(
            world
                .entity(player_entity)
                .get::<super::super::state::PlayerMarker>()
                .map(|marker| marker.player_id)
                .expect("player attack should have a player marker"),
        ),
        entity_id,
        new_pos: pos,
        kind: MovementKind::Attack,
        reliable: true,
        arg: MovementArg::basic_attack(),
        rot,
        ts,
        duration,
    }
}

fn apply_mob_motion(
    world: &mut World,
    map_entity: Entity,
    now: super::super::state::SimInstant,
    mob_entity: Entity,
    entity_id: zohar_domain::entity::EntityId,
    start_pos: LocalPos,
    end_pos: LocalPos,
    rot: Facing72,
    kind: MovementKind,
    ts: ClientTimestamp,
    duration: PacketDuration,
    next_brain: super::super::state::MobBrainState,
) -> super::super::state::PendingMovement {
    if let Some(mut transform) = world
        .entity_mut(mob_entity)
        .get_mut::<super::super::state::LocalTransform>()
    {
        transform.pos = start_pos;
        transform.rot = rot;
    }
    apply_mob_motion_state(world, mob_entity, now, start_pos, end_pos, duration);
    if let Some(mut spatial) = world
        .entity_mut(map_entity)
        .get_mut::<super::super::state::MapSpatial>()
    {
        spatial.0.update_position(entity_id, start_pos);
    }
    set_mob_brain(world, mob_entity, next_brain);

    super::super::state::PendingMovement {
        mover_player_id: None,
        entity_id,
        new_pos: end_pos,
        kind,
        reliable: false,
        arg: MovementArg::ZERO,
        rot,
        ts,
        duration,
    }
}

fn apply_mob_attack(
    world: &mut World,
    map_entity: Entity,
    now: super::super::state::SimInstant,
    mob_entity: Entity,
    entity_id: zohar_domain::entity::EntityId,
    pos: LocalPos,
    rot: Facing72,
    ts: ClientTimestamp,
    duration: PacketDuration,
    next_brain: super::super::state::MobBrainState,
) -> super::super::state::PendingMovement {
    if let Some(mut transform) = world
        .entity_mut(mob_entity)
        .get_mut::<super::super::state::LocalTransform>()
    {
        transform.pos = pos;
        transform.rot = rot;
    }
    apply_mob_motion_state(world, mob_entity, now, pos, pos, PacketDuration::ZERO);
    if let Some(mut spatial) = world
        .entity_mut(map_entity)
        .get_mut::<super::super::state::MapSpatial>()
    {
        spatial.0.update_position(entity_id, pos);
    }
    set_mob_brain(world, mob_entity, next_brain);

    super::super::state::PendingMovement {
        mover_player_id: None,
        entity_id,
        new_pos: pos,
        kind: MovementKind::Attack,
        reliable: true,
        arg: MovementArg::basic_attack(),
        rot,
        ts,
        duration,
    }
}

pub(crate) fn set_mob_brain(
    world: &mut World,
    mob_entity: Entity,
    next_brain: super::super::state::MobBrainState,
) {
    if let Some(mut brain_state) = world
        .entity_mut(mob_entity)
        .get_mut::<super::super::state::MobBrainState>()
    {
        *brain_state = next_brain;
    }
}

fn push_pending_movement(
    world: &mut World,
    map_entity: Entity,
    movement: super::super::state::PendingMovement,
) {
    let mut map_entity_ref = world.entity_mut(map_entity);
    let Some(mut pending) = map_entity_ref.get_mut::<MapPendingMovements>() else {
        return;
    };
    pending.0.push(movement);
}

fn apply_mob_motion_state(
    world: &mut World,
    mob_entity: Entity,
    now: super::super::state::SimInstant,
    start_pos: LocalPos,
    end_pos: LocalPos,
    duration: PacketDuration,
) {
    if let Some(mut motion) = world
        .entity_mut(mob_entity)
        .get_mut::<super::super::state::MobMotion>()
    {
        motion.0 = super::super::state::MobMotionState {
            segment_start_pos: start_pos,
            segment_end_pos: end_pos,
            segment_start_at: now,
            segment_end_at: now.saturating_add(
                super::super::state::SimDuration::from_packet_duration(duration),
            ),
        };
    }
}
