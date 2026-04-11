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
mod resource;
mod source;
mod stat;
mod store;
#[cfg(test)]
mod tests;
mod view;

pub use actor::{ActorImmuneFlags, ActorKind, ActorResources, ActorStatState, StatWriteError};
pub use api::{
    BootstrapStatsSync, CharacterAppearance, CharacterUpdate, GameStatsApi, SourceBundleError,
    StatDelta, StatSnapshot, StatsSync,
};
pub use balance::{default_mob_balance_rules, default_player_balance_rules};
pub use change_set::StatChangeSet;
pub use contribution::{CompiledModifier, CompiledStatContribution};
pub use player_rules::{
    HydratedPlayerStats, LevelExpEntry, LevelExpTable, PlayerClassStatsConfig,
    PlayerClassStatsTable, PlayerStatRules,
};
pub use progression::PlayerProgressionState;
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
