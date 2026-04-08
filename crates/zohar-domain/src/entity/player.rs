pub mod skill;

use crate::DbId;
use crate::coords::LocalPos;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerBaseAppearance {
    VariantA,
    VariantB,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerClass {
    Warrior,
    Ninja,
    Sura,
    Shaman,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerGender {
    Male,
    Female,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSlot {
    First,
    Second,
    Third,
    Fourth,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlayerTag {}

pub type PlayerId = DbId<PlayerTag>;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerRuntimeEpoch(i64);

impl PlayerRuntimeEpoch {
    pub const INITIAL: Self = Self(0);

    pub const fn new(raw: i64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Default for PlayerRuntimeEpoch {
    fn default() -> Self {
        Self::INITIAL
    }
}

impl From<i64> for PlayerRuntimeEpoch {
    fn from(raw: i64) -> Self {
        Self(raw)
    }
}

impl From<PlayerRuntimeEpoch> for i64 {
    fn from(epoch: PlayerRuntimeEpoch) -> Self {
        epoch.0
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerStats {
    pub stat_str: i32,
    pub stat_vit: i32,
    pub stat_dex: i32,
    pub stat_int: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSummary {
    pub id: PlayerId,
    pub slot: PlayerSlot,
    pub name: String,
    pub class: PlayerClass,
    pub gender: PlayerGender,
    pub appearance: PlayerBaseAppearance,
    pub level: i32,
    pub stats: PlayerStats,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRuntimeSnapshot {
    pub id: PlayerId,
    pub runtime_epoch: PlayerRuntimeEpoch,
    pub map_key: String,
    #[cfg_attr(feature = "admin-brp", reflect(remote = crate::coords::LocalPosReflect))]
    pub local_pos: LocalPos,
}
