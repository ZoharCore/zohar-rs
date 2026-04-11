use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum WorldS2c {
    #[brw(magic = 0x01_u8)]
    SpawnEntity {
        /// Virtual ID for the entity
        net_id: game_pkt::NetId,

        /// Rotation
        angle: f32,
        /// Absolute world position X
        x: game_pkt::WireWorldCm,
        /// Absolute world position Y
        y: game_pkt::WireWorldCm,
        /// World position Z
        #[bw(calc = 0)]
        _z_unused: i32,

        // i.e. mob npc player
        entity_type: EntityType,

        /// Race number (PlayerClassGendered or mob_proto id)
        race_num: u16,

        /// equal to character points index 17 and 19
        move_speed: u8,
        attack_speed: u8,

        state_flags: u8,

        buff_flags: u64,
    },

    #[brw(magic = 0x88_u8)]
    SetEntityDetails {
        /// Virtual ID for the entity
        net_id: game_pkt::NetId,

        name: game_pkt::EntityName,

        // cosmetics
        body_part: u16,
        wep_part: u16,
        _reserved_part: u16,
        hair_part: u16,

        empire: game_pkt::ZeroOpt<game_pkt::Empire>,

        guild_id: u32,
        level: u32,
        rank_pts: i16,
        pvp_mode: u8,

        mount_id: u32,
    },

    #[brw(magic = 0x02_u8)]
    DestroyEntity { net_id: game_pkt::NetId },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
pub enum EntityType {
    Monster = 0,
    Npc = 1,
    Stone = 2,
    Warp = 3,
    Player = 6,
    Goto = 9,
}
