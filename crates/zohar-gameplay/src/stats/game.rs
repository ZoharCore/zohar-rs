//! Game-specific stat wiring and legacy projections.
//!
//! This module intentionally exposes a small stat kernel: typed actor state,
//! source-scoped contribution ingestion, deterministic recomputation, and
//! legacy-compatible read projections.

mod actor;
mod api;
mod balance;
mod change_set;
mod contribution;
mod player_rules;
mod progression;
mod recovery;
mod resource;
mod source;
mod stat;
mod store;
#[cfg(test)]
mod tests;
mod view;

pub use actor::{ActorImmuneFlags, ActorKind, ActorResources, ActorStatState, StatWriteError};
pub use api::{
    ActorPublicState, ActorPublicStats, ActorStatsRuntime, BootstrapStatsSync,
    DrainedPlayerStatsSync, GameStatsApi, PlayerStatsRuntime, SourceBundleError, StatDelta,
    StatSnapshot, StatsSync,
};
pub use balance::{
    default_mob_balance_rules, default_player_balance_rules, exp_reward_bonus_malus_percent,
};
pub use change_set::StatChangeSet;
pub use contribution::{CompiledModifier, CompiledStatContribution};
pub use player_rules::{
    HydratedPlayerStats, LevelExpEntry, LevelExpTable, LevelExpTableError, PlayerClassStatsConfig,
    PlayerClassStatsTable, PlayerExpGainOutcome, PlayerMobExpRewardOutcome, PlayerStatRules,
    legacy_mob_exp_reward,
};
pub use progression::{LEVEL_STEPS_PER_LEVEL, PlayerProgressionState, STAT_POINT_STEPS_PER_LEVEL};
pub use recovery::{
    PlayerMovementActivity, PlayerPassiveHpRecoveryState, PlayerPassiveSpRecoveryState,
    PlayerSpRecoveryProfile, PlayerStaminaEffect, PlayerStaminaMovementOverride,
    PlayerStaminaState, PlayerStaminaTimerCommand, PlayerStatActivity,
    tick_player_passive_hp_recovery, tick_player_passive_sp_recovery, tick_player_stamina,
};
pub use resource::{QueuedRecovery, ResourceApplication, ResourceApplicationResult};
pub use source::{
    ActorStatSource, ActorViewLimits, CoreStatBlock, DeterministicGrowthVersion, MobBalanceRules,
    MobStatSource, PlayerAttackCoefficients, PlayerBalanceRules, PlayerGrowthFormula,
    PlayerResourceBonusCaps, PlayerResourceFormula, PlayerStatSource, PlayerStoredStatLimits,
    SourceSpeeds,
};
pub use view::{StatValueView, read_stat_value};
pub use zohar_domain::stat::Stat;

pub type StatModifierInstance<Source, Detail> =
    super::core::GenericModifierInstance<Source, Stat, Detail>;
pub type StatModifierLedger<Source, Detail> =
    super::core::GenericModifierLedger<Source, Stat, Detail>;
