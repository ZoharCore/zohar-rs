use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum MovementC2s {
    #[brw(magic = 0x07_u8)]
    InputMovement {
        kind: MovementKind,
        arg: u8,
        /// Rotation (0-71) - degrees / 5
        rot: u8,
        x: game_pkt::WireWorldCm,
        y: game_pkt::WireWorldCm,
        /// Client timestamp
        ts: game_pkt::WireMillis32,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum MovementS2c {
    #[brw(magic = 0x03_u8)]
    SyncEntityMovement {
        kind: MovementKind,
        arg: u8,
        /// Rotation (0-71) - degrees / 5
        rot: u8,
        net_id: game_pkt::NetId,
        x: game_pkt::WireWorldCm,
        y: game_pkt::WireWorldCm,
        /// Timestamp
        ts: game_pkt::WireMillis32,
        /// Movement duration in milliseconds
        duration: game_pkt::WireMillis32,
    },
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    Wait = 0,
    Move = 1,
    Attack = 2,
    Combo = 3,
}
