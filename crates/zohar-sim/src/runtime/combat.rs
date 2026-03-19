use bevy::prelude::*;
use tracing::debug;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;

use super::mob_brain::retaliate_mob_pack;
use super::state::{
    AttackIntentQueue, LocalTransform, MapPendingMovements, MapReplication, MobRef, NetEntityId,
    NetEntityIndex, PendingMovement, PlayerIndex, RuntimeState, SharedConfig,
};
use super::util::{packet_time_ms, rotation_from_delta};

const PLAYER_MELEE_REACH_M: f32 = 1.5;

#[derive(Debug, Clone, Copy)]
struct CombatExtentM(f32);

#[derive(Debug, Clone, Copy)]
enum PlayerAttackValidation {
    Valid {
        target_entity: Entity,
        distance_m: f32,
        max_distance_m: f32,
    },
    MissingTargetEntity,
    SelfTarget,
    NotVisible,
    MissingTargetPosition,
    OutOfRange {
        distance_m: f32,
        max_distance_m: f32,
    },
}

pub(super) fn process_attack_intents(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let (now_ms, now_ts) = {
        let state = world.resource::<RuntimeState>();
        (state.sim_time_ms, packet_time_ms(state.packet_time_start))
    };

    let player_entities: Vec<Entity> = world
        .resource::<PlayerIndex>()
        .0
        .values()
        .copied()
        .collect();

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        let (attacker_net_id, attacker_pos, attack_intents) = {
            let Some((attacker_net_id, attacker_pos)) = ({
                let entity_ref = world.entity(player_entity);
                match (
                    entity_ref.get::<NetEntityId>(),
                    entity_ref.get::<LocalTransform>(),
                ) {
                    (Some(net_id), Some(transform)) => Some((net_id.net_id, transform.pos)),
                    _ => None,
                }
            }) else {
                continue;
            };
            let intents = {
                let mut entity_mut = world.entity_mut(player_entity);
                let Some(mut queue) = entity_mut.get_mut::<AttackIntentQueue>() else {
                    continue;
                };
                std::mem::take(&mut queue.0)
            };
            (attacker_net_id, attacker_pos, intents)
        };

        for attack in attack_intents {
            let validation = validate_player_attack(
                world,
                map_entity,
                attacker_net_id,
                attacker_pos,
                attack.target,
            );
            let (target_entity, accepted_distance_m, accepted_max_distance_m) = match validation {
                PlayerAttackValidation::Valid {
                    target_entity,
                    distance_m,
                    max_distance_m,
                } => (target_entity, distance_m, max_distance_m),
                PlayerAttackValidation::MissingTargetEntity => {
                    debug!(
                        attacker = ?attacker_net_id,
                        target = ?attack.target,
                        attack_type = attack.attack_type,
                        "Rejecting player attack: target entity does not exist"
                    );
                    continue;
                }
                PlayerAttackValidation::SelfTarget => {
                    debug!(
                        attacker = ?attacker_net_id,
                        target = ?attack.target,
                        attack_type = attack.attack_type,
                        "Rejecting player attack: self target"
                    );
                    continue;
                }
                PlayerAttackValidation::NotVisible => {
                    debug!(
                        attacker = ?attacker_net_id,
                        target = ?attack.target,
                        attack_type = attack.attack_type,
                        "Rejecting player attack: target not visible in replication graph"
                    );
                    continue;
                }
                PlayerAttackValidation::MissingTargetPosition => {
                    debug!(
                        attacker = ?attacker_net_id,
                        target = ?attack.target,
                        attack_type = attack.attack_type,
                        "Rejecting player attack: target missing LocalTransform"
                    );
                    continue;
                }
                PlayerAttackValidation::OutOfRange {
                    distance_m,
                    max_distance_m,
                } => {
                    debug!(
                        attacker = ?attacker_net_id,
                        target = ?attack.target,
                        attack_type = attack.attack_type,
                        distance_m,
                        max_distance_m,
                        "Rejecting player attack: target out of range"
                    );
                    continue;
                }
            };

            if let Some(target_pos) = entity_position(world, attack.target) {
                let current_rot = world
                    .entity(player_entity)
                    .get::<LocalTransform>()
                    .map(|transform| transform.rot)
                    .unwrap_or(0);
                let rot = rotation_from_delta(attacker_pos, target_pos, current_rot);
                if let Some(mut transform) =
                    world.entity_mut(player_entity).get_mut::<LocalTransform>()
                {
                    transform.rot = rot;
                }
            }

            emit_attack_event(
                world,
                map_entity,
                attacker_net_id,
                attacker_pos,
                attack.attack_type,
                600,
                now_ts,
            );

            if world.entity(target_entity).contains::<MobRef>() {
                debug!(
                    attacker = ?attacker_net_id,
                    target = ?attack.target,
                    attack_type = attack.attack_type,
                    distance_m = accepted_distance_m,
                    max_distance_m = accepted_max_distance_m,
                    "Accepted player attack; seeding mob retaliation"
                );
                retaliate_mob_pack(world, target_entity, attacker_net_id, now_ms);
            }
        }
    }
}

fn validate_player_attack(
    world: &World,
    map_entity: Entity,
    attacker_net_id: zohar_domain::entity::EntityId,
    attacker_pos: LocalPos,
    target: zohar_domain::entity::EntityId,
) -> PlayerAttackValidation {
    let Some(target_entity) = net_entity(world, target) else {
        return PlayerAttackValidation::MissingTargetEntity;
    };
    if target_entity == net_entity(world, attacker_net_id).unwrap_or(Entity::PLACEHOLDER) {
        return PlayerAttackValidation::SelfTarget;
    }

    let target_visible = world
        .entity(map_entity)
        .get::<MapReplication>()
        .is_some_and(|replication| replication.0.is_visible(attacker_net_id, target));
    if !target_visible {
        return PlayerAttackValidation::NotVisible;
    }

    let Some(target_pos) = world
        .entity(target_entity)
        .get::<LocalTransform>()
        .map(|transform| transform.pos)
    else {
        return PlayerAttackValidation::MissingTargetPosition;
    };
    let CombatExtentM(target_extent_m) = target_combat_extent_m(world, target_entity);
    let distance_m = distance(attacker_pos, target_pos);
    let max_distance_m = PLAYER_MELEE_REACH_M + target_extent_m;
    if distance_m > max_distance_m {
        return PlayerAttackValidation::OutOfRange {
            distance_m,
            max_distance_m,
        };
    }

    PlayerAttackValidation::Valid {
        target_entity,
        distance_m,
        max_distance_m,
    }
}

fn target_combat_extent_m(world: &World, target_entity: Entity) -> CombatExtentM {
    let Some(mob_ref) = world.entity(target_entity).get::<MobRef>() else {
        return CombatExtentM(0.0);
    };
    let Some(proto) = world.resource::<SharedConfig>().mobs.get(&mob_ref.mob_id) else {
        return CombatExtentM(0.0);
    };
    CombatExtentM(proto.combat_extent_m.max(0.0))
}

fn emit_attack_event(
    world: &mut World,
    map_entity: Entity,
    entity_id: zohar_domain::entity::EntityId,
    pos: LocalPos,
    attack_type: u8,
    duration: u32,
    ts: u32,
) {
    let rot = net_entity(world, entity_id)
        .and_then(|entity| {
            world
                .entity(entity)
                .get::<LocalTransform>()
                .map(|transform| transform.rot)
        })
        .unwrap_or(0);
    if let Some(mut pending) = world
        .entity_mut(map_entity)
        .get_mut::<MapPendingMovements>()
    {
        pending.0.push(PendingMovement {
            mover_player_id: None,
            entity_id,
            new_pos: pos,
            kind: MovementKind::Attack,
            reliable: true,
            arg: attack_type,
            rot,
            ts,
            duration,
        });
    }
}

fn entity_position(world: &World, entity_id: zohar_domain::entity::EntityId) -> Option<LocalPos> {
    let entity = net_entity(world, entity_id)?;
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

fn distance(from: LocalPos, to: LocalPos) -> f32 {
    (to - from).length()
}
