use super::super::runtime::PhaseEffects;
use super::{InGameCtx, ThisPhase};
use crate::adapters::ToProtocol;
use std::time::Instant;
use zohar_protocol::game_pkt::PlayerClassGendered;
use zohar_protocol::game_pkt::ingame::world;
use zohar_protocol::game_pkt::ingame::world::EntityType;
use zohar_protocol::game_pkt::ingame::{InGameS2c, system};

pub(super) fn enter_world_effects(state: &InGameCtx<'_>) -> PhaseEffects<ThisPhase> {
    let mut effects = PhaseEffects::empty();
    let channel_id = state.ctx.channel_id.min(u8::MAX as u32) as u8;

    effects.push(InGameS2c::System(system::SystemS2c::SetServerTime {
        time: state.handshake.uptime_at(Instant::now()).into(),
    }));

    effects.push(InGameS2c::System(system::SystemS2c::SetChannelInfo {
        channel_id,
    }));

    // TODO: should this be sent as part of the map actor EnterMsg instead?
    let race_num: PlayerClassGendered = (state.player_class, state.player_gender).to_protocol();
    let race_num: u8 = race_num.into();
    // Convert meter float world coords into centimeter i32 world coords
    let (x, y) = state.spawn_pos.to_protocol();

    effects.push(InGameS2c::World(world::WorldS2c::SpawnEntity {
        net_id: state.net_id,
        angle: 0.0,
        x,
        y,
        entity_type: EntityType::Player,
        race_num: race_num as u16,
        move_speed: 100,
        attack_speed: 100,
        state_flags: 0,
        buff_flags: 0,
    }));
    effects.push(InGameS2c::World(world::WorldS2c::SetEntityDetails {
        net_id: state.net_id,
        name: state.player_name.to_string().into(),
        body_part: state.base_appearance.to_protocol() as u16,
        wep_part: 0,
        _reserved_part: 0,
        hair_part: 0,
        empire: Some(state.player_empire).to_protocol(),
        guild_id: 0,
        level: state.player_level as u32,
        rank_pts: 0,
        pvp_mode: 0,
        mount_id: 0,
    }));

    effects
}
