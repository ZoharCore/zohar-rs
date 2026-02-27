use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum SystemS2c {
    #[brw(magic = 0x6A_u8)]
    SetServerTime { time: game_pkt::WireMillis32 },

    #[brw(magic = 0x79_u8)]
    SetChannelInfo { channel_id: u8 },
}
