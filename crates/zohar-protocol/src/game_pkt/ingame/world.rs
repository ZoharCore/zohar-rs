use crate::game_pkt::{self, ingame::item::ItemTemplateId, select::PlayerBaseAppearance};
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum WorldC2s {
    #[brw(magic = 0x01_u8)]
    SignalEntitySelection { net_id: game_pkt::NetId },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum WorldS2c {
    #[brw(magic = 0x01_u8)]
    SpawnEntity {
        net_id: game_pkt::NetId,
        angle: f32,
        pos: game_pkt::WireWorldPos,
        #[bw(calc = 0)]
        _z_unused: i32,

        #[br(try_map = |wire: EntityKindCodeWire| wire.try_into())]
        #[bw(map = |entity: &EntityKindCode| EntityKindCodeWire::from(*entity))]
        entity: EntityKindCode,

        move_speed: u8,
        attack_speed: u8,

        state_flags: EntityStateFlags,
        buff_flags: EntityBuffFlags,
    },

    #[brw(magic = 0x88_u8)]
    SetEntityProfile {
        net_id: game_pkt::NetId,
        name: game_pkt::EntityName,

        parts: EntityVisualParts,

        empire: game_pkt::ZeroOpt<game_pkt::Empire>,
        guild_id: u32,
        level: u32,
        rank_pts: i16,
        pvp_mode: u8,
        mount_id: game_pkt::ZeroOpt<MobTemplateId>,
    },

    #[brw(magic = 0x13_u8)]
    SyncEntity {
        net_id: game_pkt::NetId,

        parts: EntityVisualParts,

        move_speed: u8,
        attack_speed: u8,

        state_flags: EntityStateFlags,
        buff_flags: EntityBuffFlags,

        guild_id: u32,
        rank_pts: i16,
        pvp_mode: u8,

        mount_id: game_pkt::ZeroOpt<MobTemplateId>,
    },

    #[brw(magic = 0x02_u8)]
    DestroyEntity { net_id: game_pkt::NetId },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
pub enum EntityType {
    Monster = 0,
    Npc = 1,
    Stone = 2,
    Warp = 3,
    Player = 6,
    Goto = 9,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
#[repr(u8)]
pub enum NonPlayerEntityType {
    Monster = EntityType::Monster as u8,
    Npc = EntityType::Npc as u8,
    Stone = EntityType::Stone as u8,
    Warp = EntityType::Warp as u8,
    Goto = EntityType::Goto as u8,
}

impl From<NonPlayerEntityType> for EntityType {
    fn from(value: NonPlayerEntityType) -> Self {
        match value {
            NonPlayerEntityType::Monster => EntityType::Monster,
            NonPlayerEntityType::Npc => EntityType::Npc,
            NonPlayerEntityType::Stone => EntityType::Stone,
            NonPlayerEntityType::Warp => EntityType::Warp,
            NonPlayerEntityType::Goto => EntityType::Goto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKindCode {
    Player(game_pkt::PlayerClassGendered),
    NonPlayer {
        entity_type: NonPlayerEntityType,
        race: MobTemplateId,
    },
}

impl EntityKindCode {
    pub fn entity_type(self) -> EntityType {
        match self {
            EntityKindCode::Player(_) => EntityType::Player,
            EntityKindCode::NonPlayer { entity_type, .. } => entity_type.into(),
        }
    }

    pub fn race_code(self) -> RaceCode {
        match self {
            EntityKindCode::Player(class) => EntityRace::Player(class).into(),
            EntityKindCode::NonPlayer { race, .. } => EntityRace::NonPlayer(race).into(),
        }
    }
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub struct EntityKindCodeWire {
    pub entity_type: EntityType,
    pub race: RaceCode,
}

impl TryFrom<EntityKindCodeWire> for EntityKindCode {
    type Error = String;

    fn try_from(wire: EntityKindCodeWire) -> Result<Self, Self::Error> {
        match wire.entity_type {
            EntityType::Player => game_pkt::PlayerClassGendered::try_from(wire.race.raw() as u8)
                .map(EntityKindCode::Player)
                .map_err(|_| format!("invalid player race code {}", wire.race.raw())),
            EntityType::Monster
            | EntityType::Npc
            | EntityType::Stone
            | EntityType::Warp
            | EntityType::Goto => Ok(EntityKindCode::NonPlayer {
                entity_type: NonPlayerEntityType::try_from(wire.entity_type as u8).map_err(
                    |_| format!("invalid non-player entity type {:?}", wire.entity_type),
                )?,
                race: MobTemplateId::from_raw(wire.race.raw() as u32),
            }),
        }
    }
}

impl From<EntityKindCode> for EntityKindCodeWire {
    fn from(entity: EntityKindCode) -> Self {
        Self {
            entity_type: entity.entity_type(),
            race: entity.race_code(),
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct EntityStateFlags: u8 {
        const DEAD = 1 << 0;
        const SPAWN = 1 << 1;
        const KILLER = 1 << 3;
        const PARTY = 1 << 4;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct EntityBuffFlags: u64 {
        const SPAWN = 1 << 2;
    }
}

game_pkt::impl_bitflags_binrw!(EntityStateFlags, u8);
game_pkt::impl_bitflags_binrw!(EntityBuffFlags, u64);

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct EntityVisualParts {
    pub body: BodyPartCode,

    pub weapon: game_pkt::ZeroOpt<VisualItemPartCode>,

    #[bw(calc = 0)]
    pub _reserved: u16,

    pub hair: game_pkt::ZeroOpt<HairShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPart {
    Base(PlayerBaseAppearance), // 0, 1
    Wedding,                    // 2
    Equipped(ItemTemplateId),   // 3+ equipment template id
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct VisualItemPartCode(u16);

impl VisualItemPartCode {
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub fn item_template_id(self) -> ItemTemplateId {
        ItemTemplateId::from(self.0 as u32)
    }
}

impl From<ItemTemplateId> for VisualItemPartCode {
    fn from(value: ItemTemplateId) -> Self {
        Self::from_raw(u32::from(value) as u16)
    }
}

impl From<u16> for VisualItemPartCode {
    fn from(value: u16) -> Self {
        Self::from_raw(value)
    }
}

impl From<VisualItemPartCode> for u16 {
    fn from(value: VisualItemPartCode) -> Self {
        value.raw()
    }
}

impl game_pkt::ZeroFallback for VisualItemPartCode {
    type Primitive = u16;
    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        Ok(Self::from_raw(raw))
    }
    fn into_primitive(self) -> Self::Primitive {
        self.raw()
    }
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct BodyPartCode(u16);

impl BodyPartCode {
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub fn semantics(self) -> BodyPart {
        match self.0 {
            0 => BodyPart::Base(PlayerBaseAppearance::VariantA),
            1 => BodyPart::Base(PlayerBaseAppearance::VariantB),
            2 => BodyPart::Wedding,
            v => BodyPart::Equipped(ItemTemplateId::from(v as u32)),
        }
    }
}

impl From<BodyPart> for BodyPartCode {
    fn from(value: BodyPart) -> Self {
        Self::from_raw(match value {
            BodyPart::Base(b) => u8::from(b).into(),
            BodyPart::Wedding => 2,
            BodyPart::Equipped(id) => u32::from(id) as u16,
        })
    }
}

impl From<u16> for BodyPartCode {
    fn from(value: u16) -> Self {
        Self::from_raw(value)
    }
}

impl From<BodyPartCode> for u16 {
    fn from(value: BodyPartCode) -> Self {
        value.raw()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityRace {
    Player(game_pkt::PlayerClassGendered),
    NonPlayer(MobTemplateId),
}

#[binrw::binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct RaceCode(u16);

impl RaceCode {
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub fn semantics(self, entity_type: EntityType) -> Option<EntityRace> {
        match entity_type {
            EntityType::Player => game_pkt::PlayerClassGendered::try_from(self.0 as u8)
                .ok()
                .map(EntityRace::Player),
            EntityType::Monster
            | EntityType::Npc
            | EntityType::Stone
            | EntityType::Warp
            | EntityType::Goto => Some(EntityRace::NonPlayer(MobTemplateId::from(self.0 as u32))),
        }
    }
}

impl From<EntityRace> for RaceCode {
    fn from(race: EntityRace) -> Self {
        Self::from_raw(match race {
            EntityRace::Player(p) => u8::from(p) as u16,
            EntityRace::NonPlayer(id) => u32::from(id) as u16,
        })
    }
}

impl From<u16> for RaceCode {
    fn from(value: u16) -> Self {
        Self::from_raw(value)
    }
}

impl From<RaceCode> for u16 {
    fn from(value: RaceCode) -> Self {
        value.raw()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
#[binrw::binrw]
#[brw(little)]
pub struct MobTemplateId(u32);

impl From<u32> for MobTemplateId {
    fn from(value: u32) -> Self {
        Self::from_raw(value)
    }
}

impl From<MobTemplateId> for u32 {
    fn from(value: MobTemplateId) -> Self {
        value.raw()
    }
}

impl MobTemplateId {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl game_pkt::ZeroFallback for MobTemplateId {
    type Primitive = u32;
    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        Ok(Self::from_raw(raw))
    }

    fn into_primitive(self) -> Self::Primitive {
        self.raw()
    }
}

#[binrw::binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HairShape(u16);

impl HairShape {
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl game_pkt::ZeroFallback for HairShape {
    type Primitive = u16;
    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        Ok(Self::from_raw(raw))
    }
    fn into_primitive(self) -> Self::Primitive {
        self.raw()
    }
}
