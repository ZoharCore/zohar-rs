pub mod dropped;
pub mod inventory;
pub mod shop;
pub mod trading_wip;

use binrw::binrw;

use crate::game_pkt;

pub const ITEM_SOCKET_COUNT: usize = 3;
pub const ITEM_ATTRIBUTE_COUNT: usize = 7;

pub type ItemSocketSlots = [game_pkt::ZeroOpt<ItemSocket>; ITEM_SOCKET_COUNT];
pub type ItemAttributeSlots = [game_pkt::ZeroOpt<ItemAttribute>; ITEM_ATTRIBUTE_COUNT];

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ItemTemplateId(u32);

impl ItemTemplateId {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl From<u32> for ItemTemplateId {
    fn from(value: u32) -> Self {
        Self::from_raw(value)
    }
}

impl From<ItemTemplateId> for u32 {
    fn from(value: ItemTemplateId) -> Self {
        value.raw()
    }
}

impl ItemPosition {
    pub const fn new(window: WindowKind, cell: ItemCell) -> Self {
        Self { window, cell }
    }

    pub const fn inventory(cell: ItemCell) -> Self {
        Self {
            window: WindowKind::Inventory,
            cell,
        }
    }
}

#[binrw]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemInstanceDetails {
    pub sockets: ItemSocketSlots,
    pub attributes: ItemAttributeSlots,
}

#[binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemPosition {
    pub window: WindowKind,
    pub cell: ItemCell,
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ItemCell(u16);

impl ItemCell {
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl From<u16> for ItemCell {
    fn from(value: u16) -> Self {
        Self::from_raw(value)
    }
}

impl From<ItemCell> for u16 {
    fn from(value: ItemCell) -> Self {
        value.raw()
    }
}

#[binrw::binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct WireItemCell8(u8);

impl WireItemCell8 {
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    pub fn try_from_cell(cell: ItemCell) -> Option<Self> {
        let raw = cell.raw();
        if raw <= u8::MAX as u16 {
            Some(Self(raw as u8))
        } else {
            None
        }
    }

    pub const fn cell(self) -> ItemCell {
        ItemCell::from_raw(self.0 as u16)
    }
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
pub enum WindowKind {
    Inventory = 1,
    Equipment = 2,
    Safebox = 3,
    Mall = 4,
    DragonSoulInventory = 5,
    Ground = 6,
    BeltInventory = 7,
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
pub enum ItemAttributeType {
    MaxHp = 1,
    MaxSp = 2,
    Vit = 3,
    Int = 4,
    Str = 5,
    Dex = 6,
    AttackSpeed = 7,
    MoveSpeed = 8,
    CastingSpeed = 9,
}

game_pkt::impl_zero_fallback_num_enum!(ItemAttributeType, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemAttribute {
    pub kind: ItemAttributeType,
    pub value: i16,
}

#[binrw]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemAttributeWire {
    kind: u8,
    value: i16,
}

impl game_pkt::ZeroFallback for ItemAttribute {
    type Primitive = ItemAttributeWire;

    fn is_zero_primitive(raw: Self::Primitive) -> bool {
        raw.kind == 0
    }

    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        let kind = ItemAttributeType::try_from(raw.kind).map_err(|_| "invalid item attribute")?;
        Ok(Self {
            kind,
            value: raw.value,
        })
    }

    fn into_primitive(self) -> Self::Primitive {
        ItemAttributeWire {
            kind: self.kind.into(),
            value: self.value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSocket {
    Occupied(ItemTemplateId),
    Marker(i32),
}

impl game_pkt::ZeroFallback for ItemSocket {
    type Primitive = i32;

    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        Ok(if raw > 0 {
            Self::Occupied(ItemTemplateId::from_raw(raw as u32))
        } else {
            Self::Marker(raw)
        })
    }

    fn into_primitive(self) -> Self::Primitive {
        match self {
            Self::Occupied(template_id) => template_id.raw() as i32,
            Self::Marker(raw) => raw,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SocketAttachmentRequirement {
    AnyGrade = 1,
    GoldOnly = 2,
}

#[derive(Debug, Clone)]
pub enum AttachedSocketSlot {
    Stone { stone_def_id: u32 },
    Timestamp { expires_at: game_pkt::WireMillis32 }, // LIMIT_REAL_TIME
    RemainingTime { remaining_secs: u32 },            // LIMIT_TIMER_BASED_ON_WEAR
    UsageCount { count: u32 },                        // LIMIT_REAL_TIME_START_FIRST_USE
}
