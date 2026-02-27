use binrw::binrw;

// TODO: Replace these placeholder opcodes with real trading packet definitions.

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum TradingC2s {
    #[brw(magic = 0x50_u8)]
    Reserved50,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum TradingS2c {
    #[brw(magic = 0x51_u8)]
    Reserved51,
}
