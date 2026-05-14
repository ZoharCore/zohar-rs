use binrw::binrw;

use crate::game_pkt;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum InventoryItemC2s {
    #[brw(magic = 0x0B_u8)]
    RequestUseItem { pos: super::ItemPosition },

    #[brw(magic = 0x0C_u8)]
    RequestDropItem { request: DropItemRequest },

    #[brw(magic = 0x0D_u8)]
    RequestMoveItem {
        from: super::ItemPosition,
        to: super::ItemPosition,
        qty: u8,
    },

    #[brw(magic = 0x14_u8)]
    RequestDropItemV2 { request: DropItemStackRequest },

    #[brw(magic = 0x3C_u8)]
    RequestApplyItem {
        src: super::ItemPosition,
        dest: super::ItemPosition,
    },

    #[brw(magic = 0x53_u8)]
    RequestOfferItem {
        dest: game_pkt::NetId,
        src: super::ItemPosition,
        qty: u8,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum InventoryItemS2c {
    #[brw(magic = 0x14_u8)]
    // AKA ItemSet
    SetInventoryItemOld {
        pos: super::ItemPosition,
        item_id: super::ItemTemplateId,
        qty: u8,
        details: super::ItemInstanceDetails,
    },

    #[brw(magic = 0x15_u8)]
    // AKA ItemSet2
    SetInventoryItem {
        pos: super::ItemPosition,
        item_id: super::ItemTemplateId,
        qty: u8,

        flags: u32,
        anti_flags: u32,
        highlight: u8,

        details: super::ItemInstanceDetails,
    },

    #[brw(magic = 0x16_u8)]
    NotifyItemUsed {
        pos: super::ItemPosition,
        user: game_pkt::NetId,
        dest: game_pkt::ZeroOpt<game_pkt::NetId>,
        item_id: super::ItemTemplateId,
    },

    #[brw(magic = 0x17_u8)]
    DestroyInventoryItem { pos: super::WireItemCell8 },

    #[brw(magic = 0x19_u8)]
    SetInventoryItemData {
        pos: super::ItemPosition,
        qty: u8,
        details: super::ItemInstanceDetails,
    },
}

#[binrw]
#[derive(Debug, Clone, Copy)]
pub struct DropItemRequest {
    pos: super::ItemPosition,
    gold: i32,
}

impl DropItemRequest {
    pub const fn new_item(pos: super::ItemPosition) -> Self {
        Self { pos, gold: 0 }
    }

    pub const fn new_gold(gold: i32) -> Self {
        Self {
            pos: super::ItemPosition::inventory(super::ItemCell::from_raw(0)),
            gold,
        }
    }

    pub fn intent(self) -> DropIntent {
        if self.gold > 0 {
            DropIntent::Gold { amount: self.gold }
        } else {
            DropIntent::Item {
                pos: self.pos,
                qty: None,
            }
        }
    }
}

#[binrw]
#[derive(Debug, Clone, Copy)]
pub struct DropItemStackRequest {
    pos: super::ItemPosition,
    gold: i32,
    qty: u8,
}

impl DropItemStackRequest {
    pub const fn new_item(pos: super::ItemPosition, qty: u8) -> Self {
        Self { pos, gold: 0, qty }
    }

    pub const fn new_gold(gold: i32) -> Self {
        Self {
            pos: super::ItemPosition::inventory(super::ItemCell::from_raw(0)),
            gold,
            qty: 0,
        }
    }

    pub fn intent(self) -> DropIntent {
        if self.gold > 0 {
            DropIntent::Gold { amount: self.gold }
        } else {
            DropIntent::Item {
                pos: self.pos,
                qty: Some(self.qty),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropIntent {
    Gold {
        amount: i32,
    },
    Item {
        pos: super::ItemPosition,
        qty: Option<u8>,
    },
}
