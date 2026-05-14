use binrw::binrw;

use crate::game_pkt;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum DroppedItemC2s {
    #[brw(magic = 0x0F_u8)]
    RequestPickupDroppedItem { net_id: game_pkt::NetId },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum DroppedItemS2c {
    #[brw(magic = 0x1A_u8)]
    SpawnDroppedItem {
        pos: game_pkt::WireWorldPos,
        #[bw(calc = 0)]
        _z_unused: i32,

        net_id: game_pkt::NetId,
        item_id: super::ItemTemplateId,
    },

    #[brw(magic = 0x1F_u8)]
    SetDroppedItemNametag {
        net_id: game_pkt::NetId,
        nametag: game_pkt::EntityName,
    },

    #[brw(magic = 0x1B_u8)]
    DestroyDroppedItem { net_id: game_pkt::NetId },
}
