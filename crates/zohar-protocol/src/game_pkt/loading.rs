use crate::control_pkt::{ControlC2s, ControlS2c};
use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum LoadingC2sSpecific {
    #[brw(magic = 0xF1_u8)]
    SubmitClientVersion { client: [u8; 33], version: [u8; 33] },

    #[brw(magic = 0x0A_u8)]
    SignalLoadingComplete,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum LoadingS2cSpecific {
    #[brw(magic = 0x71_u8)]
    SetMainCharacter {
        net_id: game_pkt::NetId,

        class_gender: game_pkt::PlayerClassGendered,
        #[bw(calc = 0)]
        _reserved_for_class: u8,

        name: game_pkt::EntityName,

        x: game_pkt::WireWorldCm,
        y: game_pkt::WireWorldCm,
        #[bw(calc = 0)]
        _z_unused: i32,

        empire: game_pkt::Empire,
        skill_branch: game_pkt::ZeroOpt<game_pkt::SkillBranch>,
    },

    #[brw(magic = 0x10_u8)]
    SetMainCharacterStats { stats: [u32; 255] },
}

crate::route_packets! {
    /// Client-to-server packets for Loading phase.
    pub enum LoadingC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Specific(LoadingC2sSpecific) from 0xF1 | 0x0A,
    }
}

crate::route_packets! {
    /// Server-to-client packets for Loading phase.
    pub enum LoadingS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Specific(LoadingS2cSpecific) from 0x71 | 0x10,
    }
}
