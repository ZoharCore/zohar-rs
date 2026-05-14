pub mod appearance;
pub mod coords;
pub mod entity;
pub mod item;
pub mod stat;
pub mod terrain;
pub mod util;

use std::marker::PhantomData;

pub use entity::mob::behavior::BehaviorFlags;
pub use terrain::TerrainFlags;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Empire {
    Red,
    Yellow,
    Blue,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerExitKind {
    Disconnect,
    Handoff,
}

#[repr(transparent)]
#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DbId<T>(
    i64,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))] PhantomData<T>,
);

impl<T> DbId<T> {
    pub const fn new_unchecked(raw: i64) -> Self {
        Self(raw, PhantomData)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<T> From<i64> for DbId<T> {
    fn from(raw: i64) -> Self {
        Self(raw, PhantomData)
    }
}

impl<T> From<DbId<T>> for i64 {
    fn from(id: DbId<T>) -> Self {
        id.0
    }
}

#[repr(transparent)]
#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId<T>(
    u32,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))] PhantomData<T>,
);

impl<T> DefId<T> {
    pub const fn new(raw: u32) -> Self {
        Self(raw, PhantomData)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<T> From<u32> for DefId<T> {
    fn from(raw: u32) -> Self {
        Self(raw, PhantomData)
    }
}

impl<T> From<DefId<T>> for u32 {
    fn from(id: DefId<T>) -> Self {
        id.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MapId(smol_str::SmolStr);

impl MapId {
    pub fn new(s: impl Into<smol_str::SmolStr>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for MapId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

impl From<&str> for MapId {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for MapId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
