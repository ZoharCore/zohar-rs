use bevy::prelude::World;
use zohar_domain::appearance::{
    EntityBuffFlags, EntityKind, EntityNameplate, EntityPublicEquipment, EntityPublicFlags,
    EntityPublicSocial, EntityPublicSpeeds, EntityPublicState, EntitySnapshot, EntityStateFlags,
    PlayerAppearance,
};
use zohar_domain::coords::{Facing72, LocalPos};
use zohar_domain::entity::EntityId;
use zohar_domain::entity::mob::MobKind;

use crate::runtime::common::{
    ActorLifeComp, LocalTransform, MapEmpire, MobRef, NetEntityId, NetEntityIndex,
    PlayerAppearanceComp, PlayerMarker, RuntimeState, SharedConfig,
};

pub(crate) fn make_entity_snapshot(
    world: &World,
    shared: &SharedConfig,
    target_id: EntityId,
) -> Option<EntitySnapshot> {
    let target_entity = world
        .resource::<NetEntityIndex>()
        .0
        .get(&target_id)
        .copied()?;
    let target_ref = world.entity(target_entity);

    if let (Some(_player), Some(net_id), Some(transform), Some(appearance)) = (
        target_ref.get::<PlayerMarker>(),
        target_ref.get::<NetEntityId>(),
        target_ref.get::<LocalTransform>(),
        target_ref.get::<PlayerAppearanceComp>(),
    ) {
        let snapshot = snapshot_with_life_phase(
            make_player_snapshot(net_id.net_id, transform.pos, transform.rot, &appearance.0),
            target_ref.get::<ActorLifeComp>(),
        );
        return Some(snapshot);
    }

    let mob_ref = target_ref.get::<MobRef>()?;
    let transform = target_ref.get::<LocalTransform>()?;
    let net_id = target_ref.get::<NetEntityId>()?;
    let proto = shared.mobs.get(&mob_ref.mob_id)?;

    let nameplate = if proto.mob_kind == MobKind::Npc {
        let map_empire = world
            .resource::<RuntimeState>()
            .map_entity
            .and_then(|map_entity| world.entity(map_entity).get::<MapEmpire>())
            .and_then(|emp| emp.0);

        Some(EntityNameplate {
            name: format!("[{}] {}", net_id.net_id.0, proto.name),
            empire: proto.empire.or(map_empire),
            level: 0,
        })
    } else {
        None
    };

    Some(EntitySnapshot {
        entity_id: net_id.net_id,
        facing: transform.rot,
        pos: transform.pos,
        kind: EntityKind::Mob {
            mob_id: mob_ref.mob_id,
            mob_kind: proto.mob_kind,
        },
        public_state: public_state_with_life_phase(
            mob_public_state(proto.mob_kind, proto.move_speed, proto.attack_speed),
            target_ref.get::<ActorLifeComp>(),
        ),
        nameplate,
    })
}

fn snapshot_with_life_phase(
    mut snapshot: EntitySnapshot,
    life: Option<&ActorLifeComp>,
) -> EntitySnapshot {
    snapshot.public_state = public_state_with_life_phase(snapshot.public_state, life);
    snapshot
}

pub(crate) fn make_entity_public_state(
    world: &World,
    shared: &SharedConfig,
    target_id: EntityId,
) -> Option<EntityPublicState> {
    let target_entity = world
        .resource::<NetEntityIndex>()
        .0
        .get(&target_id)
        .copied()?;
    let target_ref = world.entity(target_entity);

    if let Some(appearance) = target_ref.get::<PlayerAppearanceComp>() {
        return Some(public_state_with_life_phase(
            player_public_state(&appearance.0),
            target_ref.get::<ActorLifeComp>(),
        ));
    }

    let mob_ref = target_ref.get::<MobRef>()?;
    let proto = shared.mobs.get(&mob_ref.mob_id)?;
    Some(public_state_with_life_phase(
        mob_public_state(proto.mob_kind, proto.move_speed, proto.attack_speed),
        target_ref.get::<ActorLifeComp>(),
    ))
}

pub(crate) fn make_player_snapshot(
    net_id: EntityId,
    pos: LocalPos,
    rot: Facing72,
    appearance: &PlayerAppearance,
) -> EntitySnapshot {
    EntitySnapshot {
        entity_id: net_id,
        facing: rot,
        pos,
        kind: EntityKind::Player {
            class: appearance.class,
            gender: appearance.gender,
        },
        public_state: player_public_state(appearance),
        nameplate: Some(EntityNameplate {
            name: appearance.name.clone(),
            empire: Some(appearance.empire),
            level: appearance.level,
        }),
    }
}

fn player_public_state(appearance: &PlayerAppearance) -> EntityPublicState {
    EntityPublicState {
        equipment: EntityPublicEquipment {
            body_part: appearance.body_part,
            weapon_part: 0,
            hair_part: 0,
        },
        speeds: EntityPublicSpeeds {
            move_speed: appearance.move_speed,
            attack_speed: appearance.attack_speed,
        },
        flags: EntityPublicFlags::default(),
        social: EntityPublicSocial {
            guild_id: appearance.guild_id,
            ..Default::default()
        },
    }
}

fn mob_public_state(mob_kind: MobKind, move_speed: u8, attack_speed: u8) -> EntityPublicState {
    let flags = if mob_kind == MobKind::Stone {
        EntityPublicFlags {
            state_flags: EntityStateFlags::SPAWN,
            buff_flags: EntityBuffFlags::SPAWN,
        }
    } else {
        EntityPublicFlags::default()
    };

    EntityPublicState {
        equipment: EntityPublicEquipment::default(),
        speeds: EntityPublicSpeeds {
            move_speed,
            attack_speed,
        },
        flags,
        social: EntityPublicSocial::default(),
    }
}

fn public_state_with_life_phase(
    mut public_state: EntityPublicState,
    life: Option<&ActorLifeComp>,
) -> EntityPublicState {
    if life.is_some_and(ActorLifeComp::is_dead) {
        public_state
            .flags
            .state_flags
            .insert(EntityStateFlags::DEAD);
    }
    public_state
}
