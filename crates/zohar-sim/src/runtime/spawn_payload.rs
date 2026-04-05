use bevy::prelude::World;
use zohar_domain::appearance::{EntityDetails, EntityKind, PlayerAppearance, ShowEntity};
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::mob::MobKind;
use zohar_map_port::Facing72;

use crate::runtime::common::{
    LocalTransform, MapEmpire, MobRef, NetEntityId, NetEntityIndex, PlayerAppearanceComp,
    PlayerMarker, RuntimeState, SharedConfig,
};

pub(crate) fn make_entity_spawn_payload(
    world: &World,
    shared: &SharedConfig,
    target_id: EntityId,
) -> Option<(ShowEntity, Option<EntityDetails>)> {
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
        let (show, details) =
            make_player_spawn_payload(net_id.net_id, transform.pos, transform.rot, &appearance.0);
        return Some((show, Some(details)));
    }

    let mob_ref = target_ref.get::<MobRef>()?;
    let transform = target_ref.get::<LocalTransform>()?;
    let net_id = target_ref.get::<NetEntityId>()?;
    let proto = shared.mobs.get(&mob_ref.mob_id)?;

    let show = ShowEntity {
        entity_id: net_id.net_id,
        angle: transform.rot.get() as f32 * 5.0,
        pos: transform.pos,
        kind: EntityKind::Mob {
            mob_id: mob_ref.mob_id,
            mob_kind: proto.mob_kind,
        },
        move_speed: proto.move_speed,
        attack_speed: proto.attack_speed,
        state_flags: 0,
        buff_flags: 0,
    };

    let details = if proto.mob_kind == MobKind::Npc {
        let map_empire = world
            .resource::<RuntimeState>()
            .map_entity
            .and_then(|map_entity| world.entity(map_entity).get::<MapEmpire>())
            .and_then(|emp| emp.0);

        Some(EntityDetails {
            entity_id: net_id.net_id,
            name: format!("[{}] {}", net_id.net_id.0, proto.name),
            body_part: 0,
            wep_part: 0,
            hair_part: 0,
            empire: proto.empire.or(map_empire),
            guild_id: 0,
            level: 0,
            rank_pts: 0,
            pvp_mode: 0,
            mount_id: 0,
        })
    } else {
        None
    };

    Some((show, details))
}

pub(crate) fn make_player_spawn_payload(
    net_id: EntityId,
    pos: LocalPos,
    rot: Facing72,
    appearance: &PlayerAppearance,
) -> (ShowEntity, EntityDetails) {
    (
        ShowEntity {
            entity_id: net_id,
            angle: rot.get() as f32 * 5.0,
            pos,
            kind: EntityKind::Player {
                class: appearance.class,
                gender: appearance.gender,
            },
            move_speed: appearance.move_speed,
            attack_speed: appearance.attack_speed,
            state_flags: 0,
            buff_flags: 0,
        },
        EntityDetails {
            entity_id: net_id,
            name: appearance.name.clone(),
            body_part: appearance.body_part,
            wep_part: 0,
            hair_part: 0,
            empire: Some(appearance.empire),
            guild_id: appearance.guild_id,
            level: appearance.level,
            rank_pts: 0,
            pvp_mode: 0,
            mount_id: 0,
        },
    )
}
