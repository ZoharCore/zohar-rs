pub mod mob;
pub mod player;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u32);

impl From<u32> for EntityId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<EntityId> for u32 {
    fn from(value: EntityId) -> Self {
        value.0
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    Wait,
    Move,
    Attack,
    Combo,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MovementAnimation {
    #[default]
    Run,
    Walk,
}
