pub(crate) mod aggro;
pub(crate) mod ai;
pub(crate) mod ambient_chat;
pub(crate) mod spawn;

use bevy::prelude::*;
use std::collections::HashSet;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::mob::MobId;
use zohar_domain::entity::mob::spawn::SpawnRule;

pub(crate) use crate::runtime::action as action_pipeline;
pub(crate) use crate::runtime::common as state;
pub(crate) use crate::runtime::player::lifecycle as players;
pub(crate) use crate::runtime::rules;
pub(crate) use crate::runtime::spatial as mob_motion;
pub(crate) use crate::runtime::spatial as query;
pub(crate) use crate::runtime::spatial as util;

#[derive(Debug, Clone)]
pub(crate) struct SpawnRuleState {
    pub(crate) rule: SpawnRule,
    pub(crate) active_instances: usize,
    pub(crate) entities: HashSet<EntityId>,
    pub(crate) respawn_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MobMotionState {
    pub(crate) segment_start_pos: LocalPos,
    pub(crate) segment_end_pos: LocalPos,
    pub(crate) segment_start_at_ms: u64,
    pub(crate) segment_end_at_ms: u64,
}

#[derive(Component)]
pub(crate) struct MobMotion(pub(crate) MobMotionState);

#[derive(Component)]
pub(crate) struct MobMarker;

#[derive(Component)]
pub(crate) struct MobRef {
    pub(crate) mob_id: MobId,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MobPackId {
    pub(crate) pack_id: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MobHomeAnchor {
    pub(crate) pos: LocalPos,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobBrainMode {
    Idle,
    Pursuit,
    Return,
    AttackWindup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobAggro {
    ProvokedBy { attacker: EntityId },
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MobBrainState {
    pub(crate) mode: MobBrainMode,
    pub(crate) target: Option<EntityId>,
    pub(crate) next_attack_at_ms: u64,
    pub(crate) attack_windup_until_ms: u64,
    pub(crate) next_rethink_at_ms: u64,
    pub(crate) wander_next_decision_at_ms: u64,
    pub(crate) wander_wait_until_ms: Option<u64>,
}

#[derive(Component, Default)]
pub(crate) struct MobAggroQueue(pub(crate) Vec<MobAggro>);

impl Default for MobBrainState {
    fn default() -> Self {
        Self {
            mode: MobBrainMode::Idle,
            target: None,
            next_attack_at_ms: 0,
            attack_windup_until_ms: 0,
            next_rethink_at_ms: 0,
            wander_next_decision_at_ms: 0,
            wander_wait_until_ms: None,
        }
    }
}

#[cfg(test)]
impl MobBrainState {
    pub(crate) const fn mode(&self) -> MobBrainMode {
        self.mode
    }

    pub(crate) const fn target(&self) -> Option<EntityId> {
        self.target
    }

    pub(crate) const fn attack_windup_until_ms(&self) -> u64 {
        self.attack_windup_until_ms
    }
}

#[derive(Component)]
pub(crate) struct MobChatState {
    pub(crate) next_emit_at_ms: u64,
}
