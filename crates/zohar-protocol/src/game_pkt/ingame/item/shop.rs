use binrw::{BinWrite, binrw};

use crate::game_pkt;

pub const SHOP_TITLE_MAX_LENGTH: usize = 32;
pub type ShopTitle = game_pkt::FixedString<SHOP_TITLE_MAX_LENGTH>;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum ShopC2s {
    #[brw(magic = 0x32_u8)]
    SubmitShopWindowAction { action: PlayerShoppingAction },

    #[brw(magic = 0x37_u8)]
    SubmitOpenPrivateShop {
        title: ShopTitle,

        #[bw(calc = items.len() as u8)]
        items_len: u8,

        #[br(count = items_len)]
        items: Vec<PrivateShopItem>,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ShopS2c {
    #[brw(magic = 0x26_u8)]
    NotifyShopWindow {
        #[bw(calc = (1 + 2 + body.size()) as u16)]
        size: u16,

        body: NotifyShopBody,
    },

    #[brw(magic = 0x27_u8)]
    SetShopTitle {
        net_id: game_pkt::NetId,
        title: ShopTitle,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum PlayerShoppingAction {
    #[brw(magic = 0x00_u8)]
    EndShopping,

    #[brw(magic = 0x01_u8)]
    RequestBuy { cell: super::WireItemCell8 },

    #[brw(magic = 0x02_u8)]
    RequestSell { cell: super::WireItemCell8 },

    #[brw(magic = 0x03_u8)]
    RequestPartialSell { cell: super::WireItemCell8, qty: u8 },
}

#[binrw]
#[derive(Debug, Clone)]
pub struct PrivateShopItem {
    pub item_id: super::ItemTemplateId,
    pub qty: u8,
    pub inventory_pos: super::ItemPosition,
    pub price: u32,
    pub display_cell: super::WireItemCell8,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum NotifyShopBody {
    #[brw(magic = 0x00_u8)]
    Start {
        owner: game_pkt::NetId,
        items: [ShopItem; 40],
    },

    #[brw(magic = 0x01_u8)]
    End,

    #[brw(magic = 0x02_u8)]
    UpdateItem {
        cell: super::WireItemCell8,
        item: ShopItem,
    },

    #[brw(magic = 0x03_u8)]
    UpdatePrice { price: i32 },

    #[brw(magic = 0x04_u8)]
    Ok,

    #[brw(magic = 0x05_u8)]
    NotEnoughGold,

    #[brw(magic = 0x06_u8)]
    SoldOut6,

    #[brw(magic = 0x07_u8)]
    InventoryFull,

    #[brw(magic = 0x08_u8)]
    InvalidPos,

    #[brw(magic = 0x09_u8)]
    SoldOut,

    #[brw(magic = 0x0A_u8)]
    StartExtended {
        owner: game_pkt::NetId,

        #[bw(calc = tabs.len() as u8)]
        tabs_len: u8,

        #[br(count = tabs_len)]
        tabs: Vec<ShopTab>,
    },

    #[brw(magic = 0x0B_u8)]
    NotEnoughMoneyAlt,
}

#[binrw]
#[derive(Debug, Clone)]
pub struct ShopItem {
    pub item_id: super::ItemTemplateId,
    pub price: u32,
    pub qty: u8,
    pub cell: super::WireItemCell8,
    pub details: super::ItemInstanceDetails,
}

#[binrw]
#[derive(Debug, Clone)]
pub struct ShopTab {
    pub name: ShopTitle,
    pub currency: CurrencyKind,
    pub items: [ShopItem; 40],
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrencyKind {
    Gold = 0,
    Alternative = 1,
}

impl NotifyShopBody {
    pub fn size(&self) -> usize {
        /// A dummy writer that catches bytes and just counts them.
        #[derive(Default)]
        struct ByteCounter {
            size: u64,
            position: u64,
        }

        impl std::io::Write for ByteCounter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let len = buf.len() as u64;
                self.position += len;
                if self.position > self.size {
                    self.size = self.position;
                }
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl std::io::Seek for ByteCounter {
            fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
                self.position = match pos {
                    std::io::SeekFrom::Start(p) => p,
                    std::io::SeekFrom::End(p) => (self.size as i64 + p) as u64,
                    std::io::SeekFrom::Current(p) => (self.position as i64 + p) as u64,
                };
                Ok(self.position)
            }
        }

        let mut counter = ByteCounter::default();
        self.write_le(&mut counter)
            .expect("byte counter should never fail");

        counter.size as usize
    }
}
