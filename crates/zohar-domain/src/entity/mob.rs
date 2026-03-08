use crate::{BehaviorFlags, DefId};
use std::sync::Arc;
pub mod behavior;
pub mod spawn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobDefTag {}

pub type MobId = DefId<MobDefTag>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobKind {
    Monster,
    Npc,
    Stone,
    Portal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobRank {
    Pawn,
    SuperPawn,
    Knight,
    SuperKnight,
    Boss,
    King,
}

#[derive(Debug)]
pub struct MobPrototypeDef {
    pub mob_id: MobId,
    pub mob_kind: MobKind,
    pub name: String,
    pub rank: MobRank,
    pub level: u32,
    pub move_speed: u8,
    pub attack_speed: u8,
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
            level: 1,
            move_speed: 0,
            attack_speed: 0,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }
    }
}

pub type MobPrototype = Arc<MobPrototypeDef>;
