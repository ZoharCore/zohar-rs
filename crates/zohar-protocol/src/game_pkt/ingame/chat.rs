use crate::game_pkt;
use binrw::binrw;
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum ChatC2s {
    #[brw(magic = 0x03_u8)]
    SubmitChatMessage {
        // size includes the header: [1] magic + [2] size + [1] kind = 4
        #[bw(calc = (message.len() + 4) as u16)]
        size: u16,

        kind: ChatKind,

        #[br(count = size.saturating_sub(4) as usize)]
        message: Vec<u8>,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum ChatS2c {
    #[brw(magic = 0x04_u8)]
    NotifyChatMessage {
        // size includes the header: [1] magic + [2] size + [1] kind + [4] net_id + [1] empire = 9
        #[bw(calc = (message.len() + 9) as u16)]
        size: u16,

        kind: ChatKind,
        net_id: game_pkt::ZeroOpt<game_pkt::NetId>,
        empire: game_pkt::ZeroOpt<game_pkt::Empire>,

        #[br(count = size.saturating_sub(9) as usize)]
        message: Vec<u8>,
    },
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum ChatKind {
    Speak = 0,
    Info = 1,
    Notice = 2,
    Command = 5,
    Shout = 6,
}
