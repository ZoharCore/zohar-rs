//! Login phase packets (C2S and S2C).
//!
//! Used during the Login phase after handshake, before character selection.

use crate::control_pkt::{ControlC2s, ControlS2c};
use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum LoginC2sSpecific {
    /// Token-based login request (reuses auth server token)
    #[brw(magic = 0x6D_u8)]
    RequestTokenLogin {
        username: [u8; 31],
        token: u32,
        enc_key: [u8; 16],
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum LoginS2cSpecific {
    #[brw(magic = 0x07_u8)]
    LoginResultFail {
        #[brw(pad_size_to = 9)]
        reason: LoginFailReason,
    },

    #[brw(magic = 0x5A_u8)]
    SetAccountEmpire { empire: game_pkt::Empire },
}

crate::route_packets! {
    /// Client-to-server packets for Login phase.
    pub enum LoginC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Specific(LoginC2sSpecific) from 0x6D,
    }
}

crate::route_packets! {
    /// Server-to-client packets for Login phase.
    pub enum LoginS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Specific(LoginS2cSpecific) from 0x07 | 0x5A,
    }
}

#[binrw]
#[derive(Debug, Clone)]
pub enum LoginFailReason {
    #[brw(magic = b"WRONGPWD")]
    InvalidCredentials,

    #[brw(magic = b"ALREADY")]
    AlreadyLoggedIn,

    #[brw(magic = b"BLOCK")]
    BlockedAccount,
}
