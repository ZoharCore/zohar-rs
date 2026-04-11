use crate::{BehaviorFlags, DefId};
use std::sync::Arc;
pub mod behavior;
pub mod spawn;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobDefTag {}

pub type MobId = DefId<MobDefTag>;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobKind {
    Monster,
    Npc,
    Stone,
    Portal(PortalBehavior),
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortalBehavior {
    MapTransfer,
    LocalReposition,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobBattleType {
    Melee,
    Range,
    Magic,
    Special,
    Power,
    Tanker,
    SuperPower,
    SuperTanker,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobRank {
    Pawn,
    SuperPawn,
    Knight,
    SuperKnight,
    Boss,
    King,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug)]
pub struct MobPrototypeDef {
    pub mob_id: MobId,
    pub mob_kind: MobKind,
    pub name: String,
    pub rank: MobRank,
    pub battle_type: MobBattleType,
    pub level: u32,
    pub move_speed: u8,
    pub attack_speed: u8,
    pub aggressive_sight: u16,
    pub attack_range: u16,
    pub combat_extent_m: f32,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub bhv_flags: BehaviorFlags,
    pub empire: Option<crate::Empire>,
}

impl MobPrototypeDef {
    pub fn fallback() -> Self {
        Self {
            mob_id: MobId::new(101),
            mob_kind: MobKind::Monster,
            name: "mob_proto error".to_string(),
            rank: MobRank::Pawn,
            battle_type: MobBattleType::Melee,
            level: 1,
            move_speed: 0,
            attack_speed: 0,
            aggressive_sight: 0,
            attack_range: 150,
            combat_extent_m: 1.0,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }
    }
}

pub type MobPrototype = Arc<MobPrototypeDef>;
