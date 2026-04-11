pub mod skill;

use std::time::Duration;

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
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlayerPlaytime(Duration);

impl PlayerPlaytime {
    pub const ZERO: Self = Self(Duration::ZERO);

    pub const fn from_secs(secs: u64) -> Self {
        Self(Duration::from_secs(secs))
    }

    pub fn from_secs_i64(secs: i64) -> Self {
        Self::from_secs(secs.max(0) as u64)
    }

    pub const fn as_duration(self) -> Duration {
        self.0
    }

    pub const fn as_secs(self) -> u64 {
        self.0.as_secs()
    }

    pub fn as_secs_i64(self) -> i64 {
        i64::try_from(self.as_secs()).unwrap_or(i64::MAX)
    }

    pub fn whole_minutes_u32(self) -> u32 {
        u32::try_from(self.as_secs() / 60).unwrap_or(u32::MAX)
    }

    pub fn saturating_add(self, elapsed: Duration) -> Self {
        Self(self.0.saturating_add(elapsed))
    }
}

impl From<Duration> for PlayerPlaytime {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<PlayerPlaytime> for Duration {
    fn from(value: PlayerPlaytime) -> Self {
        value.0
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
    pub playtime: PlayerPlaytime,
    pub stats: PlayerStats,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreStatKind {
    St,
    Ht,
    Dx,
    Iq,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoreStatAllocations {
    pub allocated_str: i32,
    pub allocated_vit: i32,
    pub allocated_dex: i32,
    pub allocated_int: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerGameplayBootstrap {
    pub player_id: PlayerId,
    pub class: PlayerClass,
    pub level: i32,
    pub exp_in_level: i64,
    pub core_stat_allocations: CoreStatAllocations,
    pub stat_reset_count: i32,
    pub current_hp: Option<i32>,
    pub current_sp: Option<i32>,
    pub current_stamina: Option<i32>,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProgressionSnapshot {
    pub core_stat_allocations: CoreStatAllocations,
    pub stat_reset_count: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRuntimeSnapshot {
    pub id: PlayerId,
    pub runtime_epoch: PlayerRuntimeEpoch,
    pub map_key: String,
    #[cfg_attr(feature = "admin-brp", reflect(remote = crate::coords::LocalPosReflect))]
    pub local_pos: LocalPos,
    pub playtime: PlayerPlaytime,
    pub current_hp: Option<i32>,
    pub current_sp: Option<i32>,
    pub current_stamina: Option<i32>,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    pub runtime: PlayerRuntimeSnapshot,
    pub progression: PlayerProgressionSnapshot,
}

impl PlayerSnapshot {
    pub fn player_id(&self) -> PlayerId {
        self.runtime.id
    }

    pub fn with_runtime_location(mut self, map_key: String, local_pos: LocalPos) -> Self {
        self.runtime.map_key = map_key;
        self.runtime.local_pos = local_pos;
        self
    }
}
