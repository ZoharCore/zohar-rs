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

use crate::runtime::time::SimInstant;

pub(crate) use crate::runtime::action as action_pipeline;
pub(crate) use crate::runtime::common as state;
pub(crate) use crate::runtime::player::lifecycle as players;
pub(crate) use crate::runtime::rules;
pub(crate) use crate::runtime::spatial as mob_motion;
pub(crate) use crate::runtime::spatial as query;
pub(crate) use crate::runtime::spatial as util;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(from_reflect = false))]
#[derive(Debug, Clone)]
pub(crate) struct SpawnRuleState {
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub(crate) rule: SpawnRule,
    pub(crate) active_instances: usize,
    pub(crate) entities: HashSet<EntityId>,
    pub(crate) respawn_at: Option<SimInstant>,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct MobMotionState {
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) segment_start_pos: LocalPos,
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) segment_end_pos: LocalPos,
    pub(crate) segment_start_at: SimInstant,
    pub(crate) segment_end_at: SimInstant,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MobMotion(pub(crate) MobMotionState);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MobMarker;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MobRef {
    pub(crate) mob_id: MobId,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MobPackId {
    pub(crate) pack_id: u32,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MobHomeAnchor {
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) pos: LocalPos,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobBrainMode {
    Idle,
    Pursuit,
    Return,
    AttackWindup,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobAggro {
    ProvokedBy { attacker: EntityId },
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MobBrainState {
    pub(crate) mode: MobBrainMode,
    pub(crate) target: Option<EntityId>,
    pub(crate) next_attack_at: SimInstant,
    pub(crate) attack_windup_until: SimInstant,
    pub(crate) next_rethink_at: SimInstant,
    pub(crate) wander_next_decision_at: SimInstant,
    pub(crate) wander_wait_until: Option<SimInstant>,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct MobAggroQueue(pub(crate) Vec<MobAggro>);

impl Default for MobBrainState {
    fn default() -> Self {
        Self {
            mode: MobBrainMode::Idle,
            target: None,
            next_attack_at: SimInstant::ZERO,
            attack_windup_until: SimInstant::ZERO,
            next_rethink_at: SimInstant::ZERO,
            wander_next_decision_at: SimInstant::ZERO,
            wander_wait_until: None,
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

    pub(crate) const fn attack_windup_until(&self) -> SimInstant {
        self.attack_windup_until
    }
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MobChatState {
    pub(crate) next_emit_at: SimInstant,
}
