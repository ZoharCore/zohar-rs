//! Select phase packets (C2S and S2C).
//!
//! Used during character selection phase.

use crate::control_pkt::{ControlC2s, ControlS2c};
use crate::game_pkt;
use crate::game_pkt::FixedString;
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub const MAX_PLAYER_SLOTS: usize = 4;
const DELETE_CODE_MAX_LENGTH: usize = 7;
const GUILD_NAME_MAX_LENGTH: usize = 13;
pub type GuildName = FixedString<GUILD_NAME_MAX_LENGTH>;

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Clone)]
pub enum SelectC2sSpecific {
    #[brw(magic = 0x5A_u8)]
    SubmitEmpireChoice { empire: game_pkt::Empire },

    #[brw(magic = 0x04_u8)]
    RequestCreatePlayer {
        slot: PlayerSelectSlot,
        name: [u8; game_pkt::PLAYER_NAME_MAX_LENGTH],
        class_gender: game_pkt::PlayerClassGendered,
        _reserved: u8,
        appearance: PlayerBaseAppearance,
        stat_vit: u8,
        stat_int: u8,
        stat_str: u8,
        stat_dex: u8,
    },

    #[brw(magic = 0x05_u8)]
    RequestDeletePlayer {
        slot: PlayerSelectSlot,
        code: [u8; DELETE_CODE_MAX_LENGTH],
        _reserved: u8,
    },

    #[brw(magic = 0x06_u8)]
    SubmitPlayerChoice { slot: PlayerSelectSlot },
}

#[binrw::binrw]
#[bw(little)]
#[br(little)]
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum SelectS2cSpecific {
    #[brw(magic = 0x5A_u8)]
    SetAccountEmpire { empire: game_pkt::Empire },

    #[brw(magic = 0x20_u8)]
    SetPlayerChoices {
        players: [Player; MAX_PLAYER_SLOTS],
        guild_ids: [u32; MAX_PLAYER_SLOTS],
        guild_names: [GuildName; MAX_PLAYER_SLOTS],

        #[bw(calc = 0)]
        _unknown: u64,
    },

    #[brw(magic = 0x08_u8)]
    CreatePlayerResultOk {
        slot: PlayerSelectSlot,
        new_player: Player,
    },
    #[brw(magic = 0x09_u8)]
    CreatePlayerResultFail { error: CreatePlayerError },

    #[brw(magic = 0x0A_u8)]
    DeletePlayerResultOk { slot: PlayerSelectSlot },
    #[brw(magic = 0x0B_u8)]
    DeletePlayerResultFail,
}

crate::route_packets! {
    /// Client-to-server packets for Select phase.
    pub enum SelectC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Specific(SelectC2sSpecific) from 0x5A | 0x04 | 0x05 | 0x06,
    }
}

crate::route_packets! {
    /// Server-to-client packets for Select phase.
    pub enum SelectS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Specific(SelectS2cSpecific) from 0x5A | 0x20 | 0x08 | 0x09 | 0x0A | 0x0B,
    }
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum PlayerSelectSlot {
    First = 0,
    Second = 1,
    Third = 2,
    Fourth = 3,
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum PlayerBaseAppearance {
    VariantA = 0,
    VariantB = 1,
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum CreatePlayerError {
    GenericFailure = 0,
    NameAlreadyExists = 1,
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct Player {
    pub db_id: u32,
    pub name: game_pkt::EntityName,
    pub class_gendered: game_pkt::PlayerClassGendered,
    pub level: u8,
    pub playtime_minutes: u32,
    pub stat_str: u8,
    pub stat_vit: u8,
    pub stat_dex: u8,
    pub stat_int: u8,
    pub body_part: u16,
    pub changed_name: u8,
    pub hair_part: u16,
    #[bw(calc = 0)]
    pub _reserved: u32,
    pub pos_x: game_pkt::WireWorldCm,
    pub pos_y: game_pkt::WireWorldCm,
    pub server_addr: game_pkt::WireServerAddr,
    pub skill_branch: game_pkt::ZeroOpt<game_pkt::SkillBranch>,
}

impl Player {
    pub fn empty() -> Self {
        Self {
            db_id: 0,
            name: Default::default(),
            class_gendered: game_pkt::PlayerClassGendered::WarriorMale,
            level: 0,
            playtime_minutes: 0,
            stat_str: 0,
            stat_vit: 0,
            stat_dex: 0,
            stat_int: 0,
            body_part: 0,
            changed_name: 0,
            hair_part: 0,
            pos_x: 0.into(),
            pos_y: 0.into(),
            server_addr: game_pkt::WireServerAddr::UNROUTABLE,
            skill_branch: game_pkt::ZeroOpt::none(),
        }
    }
}
