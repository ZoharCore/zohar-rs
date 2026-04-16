use crate::ContentCoords;
use crate::adapters::ToProtocol;
use tracing::warn;
use zohar_domain::MapId;
use zohar_domain::appearance::{EntityDetails, ShowEntity};
use zohar_domain::entity::EntityId;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::world::WorldS2c;

pub(super) fn encode_entity_spawn(
    show: ShowEntity,
    details: Option<EntityDetails>,
    map_id: MapId,
    coords: &ContentCoords,
) -> Vec<InGameS2c> {
    let Some(world_pos) = coords.local_to_world(map_id, show.pos) else {
        warn!(?show, "unable to encode spawn due to invalid position");
        return Vec::new();
    };

    let net_id = show.entity_id.to_protocol();

    let (entity_type, race_num) = show.kind.to_protocol();
    let show_pkt = WorldS2c::SpawnEntity {
        net_id,
        angle: show.angle,
        pos: world_pos.to_protocol(),
        entity_type,
        race_num,
        move_speed: show.move_speed,
        attack_speed: show.attack_speed,
        state_flags: show.state_flags,
        buff_flags: show.buff_flags,
    };

    let mut out = vec![show_pkt.into()];

    if let Some(details) = details {
        let details_pkt = WorldS2c::SetEntityDetails {
            net_id,
            name: details.name.into(),
            body_part: details.body_part,
            wep_part: details.wep_part,
            hair_part: details.hair_part,
            empire: details.empire.to_protocol(),
            guild_id: details.guild_id,
            level: details.level,
            rank_pts: details.rank_pts,
            pvp_mode: details.pvp_mode,
            mount_id: details.mount_id,
        };

        out.push(details_pkt.into());
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
