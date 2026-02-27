use binrw::binrw;

// TODO: Replace these placeholder opcodes with real fishing packet definitions.

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum FishingC2s {
    #[brw(magic = 0x70_u8)]
    Reserved70,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum FishingS2c {
    #[brw(magic = 0x71_u8)]
    Reserved71,
}
