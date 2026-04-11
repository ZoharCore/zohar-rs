use std::cmp::Ord;
use std::collections::BTreeSet;

use crate::stats::game::stat::{StatExt, StatRole};

use super::progression::PlayerProgressionState;
use thiserror::Error;

use super::Stat;
use super::store::PointValueStore;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    Player,
    Mob,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorResources {
    pub hp: i32,
    pub sp: i32,
    pub stamina: i32,
    pub hp_recovery: i32,
    pub sp_recovery: i32,
}

impl ActorResources {
    pub fn clamp_to_caps(&mut self, computed: &PointValueStore) {
        self.hp = self.hp.clamp(0, computed.get(Stat::MaxHp));
        self.sp = self.sp.clamp(0, computed.get(Stat::MaxSp));
        self.stamina = self.stamina.clamp(0, computed.get(Stat::MaxStamina));
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorImmuneFlags {
    pub stun: bool,
    pub slow: bool,
    pub fall: bool,
}

impl ActorImmuneFlags {
    pub fn or(self, other: Self) -> Self {
        Self {
            stun: self.stun || other.stun,
            slow: self.slow || other.slow,
            fall: self.fall || other.fall,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorRuntimeState {
    pub immune_flags: ActorImmuneFlags,
    pub external_immune_flags: ActorImmuneFlags,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StatWriteError {
    #[error("{stat:?} is not a stored stat")]
    NotStored { stat: Stat },
    #[error("{stat:?} must be updated via the typed progression API")]
    RequiresTypedProgression { stat: Stat },
    #[error("{stat:?} value {value} is outside the allowed range {min:?}..={max}")]
    OutOfRange {
        stat: Stat,
        value: i32,
        min: Option<i32>,
        max: i32,
    },
    #[error("{stat:?} is not a resource stat")]
    NotResource { stat: Stat },
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorStatState<Source = (), Detail = ()>
where
    Source: Ord + Copy,
{
    kind: ActorKind,
    stable_id: Option<u64>,
    progression: PlayerProgressionState,
    base: PointValueStore,
    computed: PointValueStore,
    resources: ActorResources,
    runtime: ActorRuntimeState,
    modifiers: super::StatModifierLedger<Source, Detail>,
    explicit_changes: BTreeSet<Stat>,
    dirty: bool,
    runtime_dirty: bool,
}

impl<Source, Detail> ActorStatState<Source, Detail>
where
    Source: Ord + Copy,
{
    pub fn new(kind: ActorKind) -> Self {
        Self {
            kind,
            stable_id: None,
            progression: PlayerProgressionState::default(),
            base: PointValueStore::new(),
            computed: PointValueStore::new(),
            resources: ActorResources::default(),
            runtime: ActorRuntimeState::default(),
            modifiers: super::StatModifierLedger::default(),
            explicit_changes: BTreeSet::new(),
            dirty: true,
            runtime_dirty: true,
        }
    }

    pub(crate) fn kind(&self) -> ActorKind {
        self.kind
    }

    pub(crate) fn base(&self) -> &PointValueStore {
        &self.base
    }

    pub(crate) fn computed(&self) -> &PointValueStore {
        &self.computed
    }

    pub(crate) fn resources(&self) -> ActorResources {
        self.resources
    }

    pub(crate) fn stable_id(&self) -> Option<u64> {
        self.stable_id
    }

    pub(crate) fn progression(&self) -> PlayerProgressionState {
        self.progression
    }

    pub(crate) fn runtime(&self) -> &ActorRuntimeState {
        &self.runtime
    }

    pub(crate) fn modifiers(&self) -> &super::StatModifierLedger<Source, Detail> {
        &self.modifiers
    }

    pub(crate) fn modifiers_mut(&mut self) -> &mut super::StatModifierLedger<Source, Detail> {
        self.dirty = true;
        &mut self.modifiers
    }

    pub(crate) fn set_stored_stat(&mut self, stat: Stat, value: i32) -> Result<(), StatWriteError> {
        if matches!(
            stat,
            Stat::Level | Stat::Exp | Stat::NextExp | Stat::LevelStep
        ) {
            return Err(StatWriteError::RequiresTypedProgression { stat });
        }
        if !matches!(stat.role(), StatRole::Persistent) {
            return Err(StatWriteError::NotStored { stat });
        }
        if self.base.get(stat) != value {
            self.base.set(stat, value);
            self.record_explicit_change(stat);
        }

        Ok(())
    }

    pub(crate) fn set_player_progression(&mut self, progression: PlayerProgressionState) {
        let progression = progression.normalized();
        self.progression = progression;
        self.set_base_projection_stat(Stat::Level, progression.level);
        self.set_base_projection_stat(Stat::Exp, clamp_u32_to_i32(progression.exp_in_level));
        self.set_base_projection_stat(
            Stat::NextExp,
            clamp_u32_to_i32(progression.next_exp_in_level),
        );
        self.set_base_projection_stat(Stat::LevelStep, progression.quarter_chunks_level_step());
    }

    #[cfg(test)]
    pub(crate) fn set_computed_stat(&mut self, stat: Stat, value: i32) {
        if self.computed.get(stat) != value {
            self.computed.set(stat, value);
            self.record_explicit_change(stat);
        }
    }

    pub(crate) fn set_resource_stat(
        &mut self,
        stat: Stat,
        value: i32,
    ) -> Result<(), StatWriteError> {
        let slot = match stat {
            Stat::Hp => &mut self.resources.hp,
            Stat::Sp => &mut self.resources.sp,
            Stat::Stamina => &mut self.resources.stamina,
            Stat::HpRecovery => &mut self.resources.hp_recovery,
            Stat::SpRecovery => &mut self.resources.sp_recovery,
            _ => return Err(StatWriteError::NotResource { stat }),
        };

        if *slot != value {
            *slot = value;
            self.record_explicit_change(stat);
        }

        Ok(())
    }

    pub(crate) fn set_external_immune_flags(&mut self, flags: ActorImmuneFlags) {
        if self.runtime.external_immune_flags != flags {
            self.runtime.external_immune_flags = flags;
            self.mark_runtime_dirty();
        }
    }

    pub(crate) fn set_stable_id(&mut self, stable_id: Option<u64>) {
        if self.stable_id != stable_id {
            self.stable_id = stable_id;
            self.dirty = true;
        }
    }

    pub(crate) fn set_derived_immune_flags(&mut self, flags: ActorImmuneFlags) {
        if self.runtime.immune_flags != flags {
            self.runtime.immune_flags = flags;
            self.runtime_dirty = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn overwrite_computed_from_base(&mut self) {
        self.computed = self.base.clone();
        self.dirty = false;
    }

    pub(crate) fn replace_computed(&mut self, computed: PointValueStore) {
        self.computed = computed;
        self.dirty = true;
    }

    pub(crate) fn clamp_resources_to_computed_caps(&mut self) {
        self.resources.clamp_to_caps(&self.computed);
        self.dirty = true;
    }

    pub(crate) fn replace_modifier_source(
        &mut self,
        source: Source,
        modifiers: impl IntoIterator<Item = super::StatModifierInstance<Source, Detail>>,
    ) {
        self.modifiers_mut().replace_source(source, modifiers);
    }

    pub(crate) fn remove_modifier_source(&mut self, source: Source) -> bool {
        self.modifiers_mut().remove_source(source).is_some()
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty || self.runtime_dirty
    }

    pub(crate) fn take_explicit_changes(&mut self) -> BTreeSet<Stat> {
        std::mem::take(&mut self.explicit_changes)
    }

    pub(crate) fn take_runtime_dirty(&mut self) -> bool {
        std::mem::take(&mut self.runtime_dirty)
    }

    fn record_explicit_change(&mut self, stat: Stat) {
        self.explicit_changes.insert(stat);
        self.dirty = true;
    }

    fn set_base_projection_stat(&mut self, stat: Stat, value: i32) {
        if self.base.get(stat) != value {
            self.base.set(stat, value);
            self.record_explicit_change(stat);
        }
    }

    fn mark_runtime_dirty(&mut self) {
        self.runtime_dirty = true;
        self.dirty = true;
    }
}

fn clamp_u32_to_i32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}
