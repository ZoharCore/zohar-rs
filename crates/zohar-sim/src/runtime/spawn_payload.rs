use bevy::prelude::World;
use zohar_domain::appearance::{
    EntityKind, EntityNameplate, EntityPublicEquipment, EntityPublicFlags, EntityPublicSocial,
    EntityPublicSpeeds, EntityPublicState, EntitySnapshot, PlayerAppearance,
};
use zohar_domain::coords::{Facing72, LocalPos};
use zohar_domain::entity::EntityId;
use zohar_domain::entity::mob::MobKind;

use crate::runtime::common::{
    LocalTransform, MapEmpire, MobRef, NetEntityId, NetEntityIndex, PlayerAppearanceComp,
    PlayerMarker, RuntimeState, SharedConfig,
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
        return Some(make_player_snapshot(
            net_id.net_id,
            transform.pos,
            transform.rot,
            &appearance.0,
        ));
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
        public_state: mob_public_state(proto.move_speed, proto.attack_speed),
        nameplate,
    })
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
        return Some(player_public_state(&appearance.0));
    }

    let mob_ref = target_ref.get::<MobRef>()?;
    let proto = shared.mobs.get(&mob_ref.mob_id)?;
    Some(mob_public_state(proto.move_speed, proto.attack_speed))
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

fn mob_public_state(move_speed: u8, attack_speed: u8) -> EntityPublicState {
    EntityPublicState {
        equipment: EntityPublicEquipment::default(),
        speeds: EntityPublicSpeeds {
            move_speed,
            attack_speed,
        },
        flags: EntityPublicFlags::default(),
        social: EntityPublicSocial::default(),
    }
}
