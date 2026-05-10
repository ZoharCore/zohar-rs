use crate::ContentCoords;
use crate::adapters::ToProtocol;
use tracing::warn;
use zohar_domain::MapId;
use zohar_domain::appearance::{
    EntityBuffFlags, EntityPublicState, EntitySnapshot, EntityStateFlags,
};
use zohar_domain::entity::EntityId;
use zohar_domain::util::FlagsMapper;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::world::{self, WorldS2c};

pub(super) fn encode_entity_spawn(
    snapshot: EntitySnapshot,
    map_id: MapId,
    coords: &ContentCoords,
) -> Vec<InGameS2c> {
    let Some(world_pos) = coords.local_to_world(&map_id, snapshot.pos) else {
        warn!(?snapshot, "unable to encode spawn due to invalid position");
        return Vec::new();
    };

    let net_id = snapshot.entity_id.to_protocol();

    let (entity_type, race_num) = snapshot.kind.to_protocol();
    let show_pkt = WorldS2c::SpawnEntity {
        net_id,
        angle: angle_from_facing(snapshot.facing),
        pos: world_pos.to_protocol(),
        entity_type,
        race_num,
        move_speed: snapshot.public_state.speeds.move_speed,
        attack_speed: snapshot.public_state.speeds.attack_speed,
        state_flags: encode_entity_state_flags(snapshot.public_state.flags.state_flags),
        buff_flags: encode_entity_buff_flags(snapshot.public_state.flags.buff_flags),
    };

    let mut out = vec![show_pkt.into()];

    if let Some(nameplate) = snapshot.nameplate {
        let profile_pkt = WorldS2c::SetEntityProfile {
            net_id,
            name: nameplate.name.into(),
            body_part: snapshot.public_state.equipment.body_part,
            wep_part: snapshot.public_state.equipment.weapon_part,
            hair_part: snapshot.public_state.equipment.hair_part,
            empire: nameplate.empire.to_protocol(),
            guild_id: snapshot.public_state.social.guild_id,
            level: nameplate.level,
            rank_pts: snapshot.public_state.social.rank_pts,
            pvp_mode: snapshot.public_state.social.pvp_mode,
            mount_id: snapshot.public_state.social.mount_id,
        };

        out.push(profile_pkt.into());
    }

    out
}

pub(super) fn encode_entity_despawn(entity_id: EntityId) -> Vec<InGameS2c> {
    vec![
        WorldS2c::DestroyEntity {
            net_id: entity_id.to_protocol(),
        }
        .into(),
    ]
}

pub(super) fn encode_entity_public_state_change(
    entity_id: EntityId,
    public_state: EntityPublicState,
) -> Vec<InGameS2c> {
    vec![
        WorldS2c::SyncEntity {
            net_id: entity_id.to_protocol(),
            body_part: public_state.equipment.body_part,
            wep_part: public_state.equipment.weapon_part,
            hair_part: public_state.equipment.hair_part,
            move_speed: public_state.speeds.move_speed,
            attack_speed: public_state.speeds.attack_speed,
            state_flags: encode_entity_state_flags(public_state.flags.state_flags),
            buff_flags: encode_entity_buff_flags(public_state.flags.buff_flags),
            guild_id: public_state.social.guild_id,
            rank_pts: public_state.social.rank_pts,
            pvp_mode: public_state.social.pvp_mode,
            mount_id: public_state.social.mount_id,
        }
        .into(),
    ]
}

fn angle_from_facing(facing: zohar_domain::coords::Facing72) -> f32 {
    facing.get() as f32 * 5.0
}

fn encode_entity_state_flags(flags: EntityStateFlags) -> world::EntityStateFlags {
    const MAPPER: FlagsMapper<EntityStateFlags, world::EntityStateFlags> = FlagsMapper::new(&[
        (EntityStateFlags::DEAD, world::EntityStateFlags::DEAD),
        (EntityStateFlags::SPAWN, world::EntityStateFlags::SPAWN),
    ]);

    MAPPER.map(flags)
}

fn encode_entity_buff_flags(flags: EntityBuffFlags) -> world::EntityBuffFlags {
    const MAPPER: FlagsMapper<EntityBuffFlags, world::EntityBuffFlags> =
        FlagsMapper::new(&[(EntityBuffFlags::SPAWN, world::EntityBuffFlags::SPAWN)]);

    MAPPER.map(flags)
}
