use binrw::binrw;

// TODO: Replace these placeholder opcodes with real guild packet definitions.

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum GuildC2s {
    #[brw(magic = 0x60_u8)]
    Reserved60,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum GuildS2c {
    #[brw(magic = 0x61_u8)]
    Reserved61,
}
