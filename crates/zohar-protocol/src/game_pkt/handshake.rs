//! Handshake phase packets (game server).

use crate::control_pkt::{ControlC2s, ControlS2c};
use binrw::binrw;
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum HandshakeGameC2sSpecific {
    #[brw(magic = 0xCE_u8)]
    FetchChannelList,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum HandshakeGameS2cSpecific {
    #[brw(magic = 0xD2_u8)]
    ChannelListResponse {
        #[bw(calc = statuses.len() as u32)]
        size: u32,

        #[br(count = size)]
        statuses: Vec<ServerInfo>,
        is_ok: u8,
    },
}

crate::route_packets! {
    /// Client-to-server packets for handshake phase (game server).
    pub enum HandshakeGameC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Specific(HandshakeGameC2sSpecific) from 0xCE,
    }
}

crate::route_packets! {
    /// Server-to-client packets for handshake phase (game server).
    pub enum HandshakeGameS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Specific(HandshakeGameS2cSpecific) from 0xD2,
    }
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, IntoPrimitive, TryFromPrimitive)]
pub enum ServerStatus {
    Offline = 0,
    Online = 1,
    OnlineBusy = 2,
    Full = 3,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub srv_port: u16,
    pub status: ServerStatus,
}
