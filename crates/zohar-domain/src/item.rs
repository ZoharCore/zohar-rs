use crate::DefId;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemDefTag {}

pub type ItemDefId = DefId<ItemDefTag>;
