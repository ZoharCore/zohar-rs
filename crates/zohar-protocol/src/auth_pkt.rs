use crate::control_pkt::{ControlC2s, ControlS2c};
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum AuthC2sSpecific {
    #[brw(magic = 0x6F_u8)]
    RequestPasswordLogin {
        username: [u8; 31],
        password: [u8; 17],
        enc_key: [u8; 16],
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum AuthS2cSpecific {
    #[brw(magic = 0x07_u8)]
    LoginResultFail {
        #[brw(pad_size_to = 13)]
        reason: LoginFailureReason,
    },

    #[brw(magic = 0x96_u8)]
    LoginResultOk { token: u32, is_ok: u8 },
}

// Handshake phase
crate::route_packets! {
    pub enum HandshakeAuthC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
    }
}

crate::route_packets! {
    pub enum HandshakeAuthS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
    }
}

// Auth phase
crate::route_packets! {
    pub enum AuthC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Specific(AuthC2sSpecific) from 0x6F,
    }
}

crate::route_packets! {
    pub enum AuthS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Specific(AuthS2cSpecific) from 0x07 | 0x96,
    }
}

#[binrw]
#[derive(Debug, Clone)]
pub enum LoginFailureReason {
    #[brw(magic = b"WRONGPWD")]
    InvalidCredentials,

    #[brw(magic = b"FULL")]
    ServerAtCapacity,
}
