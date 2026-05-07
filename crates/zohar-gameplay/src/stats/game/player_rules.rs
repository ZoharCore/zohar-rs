use std::collections::BTreeMap;

use thiserror::Error;
use zohar_domain::entity::player::{
    CoreStatAllocations, PlayerClass as DomainPlayerClass, PlayerGameplayBootstrap, PlayerStats,
};

use super::{
    ActorKind, ActorStatSource, ActorStatState, BootstrapStatsSync, CoreStatBlock, GameStatsApi,
    LEVEL_STEPS_PER_LEVEL, PlayerProgressionState, STAT_POINT_STEPS_PER_LEVEL, Stat,
    exp_reward_bonus_malus_percent,
};

const MAX_LEVEL_STAT_POINTS_EARNING: i32 = 91;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerClassStatsConfig {
    pub base_stats: CoreStatBlock,
    pub stat_source: ActorStatSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerClassStatsTable(Vec<(DomainPlayerClass, PlayerClassStatsConfig)>);

impl PlayerClassStatsTable {
    pub fn new(entries: Vec<(DomainPlayerClass, PlayerClassStatsConfig)>) -> Self {
        Self(entries)
    }

    pub fn stat_source_for_class(&self, class: DomainPlayerClass) -> Option<ActorStatSource> {
        self.0
            .iter()
            .find(|(candidate, _)| *candidate == class)
            .map(|(_, entry)| entry.stat_source)
    }

    pub fn resolve_player_stats(
        &self,
        class: DomainPlayerClass,
        allocations: CoreStatAllocations,
    ) -> Option<PlayerStats> {
        let base = self
            .0
            .iter()
            .find(|(candidate, _)| *candidate == class)
            .map(|(_, entry)| entry.base_stats)?;

        Some(PlayerStats {
            stat_str: base.st + allocations.allocated_str,
            stat_vit: base.ht + allocations.allocated_vit,
            stat_dex: base.dx + allocations.allocated_dex,
            stat_int: base.iq + allocations.allocated_int,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelExpEntry {
    pub level: i32,
    pub next_exp: i64,
    pub death_loss_pct: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelExpTable(BTreeMap<i32, LevelExpEntry>);

impl LevelExpTable {
    pub fn new(entries: impl IntoIterator<Item = LevelExpEntry>) -> Self {
        Self::try_new(entries).expect("level exp table must be valid")
    }

    pub fn try_new(
        entries: impl IntoIterator<Item = LevelExpEntry>,
    ) -> Result<Self, LevelExpTableError> {
        let mut by_level = BTreeMap::new();
        for entry in entries {
            if entry.level < 1 {
                return Err(LevelExpTableError::InvalidLevel { level: entry.level });
            }
            if entry.next_exp <= 0 {
                return Err(LevelExpTableError::InvalidNextExp {
                    level: entry.level,
                    next_exp: entry.next_exp,
                });
            }
            if by_level.insert(entry.level, entry).is_some() {
                return Err(LevelExpTableError::DuplicateLevel { level: entry.level });
            }
        }

        if by_level.is_empty() {
            return Err(LevelExpTableError::Empty);
        }
        if !by_level.contains_key(&1) {
            return Err(LevelExpTableError::MissingLevel { level: 1 });
        }

        let mut expected = 1;
        for level in by_level.keys().copied() {
            if level != expected {
                return Err(LevelExpTableError::MissingLevel { level: expected });
            }
            expected += 1;
        }

        Ok(Self(by_level))
    }

    pub fn max_level(&self) -> Option<i32> {
        self.0.last_key_value().map(|(level, _)| *level)
    }

    pub fn entry_for_level(&self, level: i32) -> Option<LevelExpEntry> {
        self.0.get(&level).copied()
    }

    pub fn progression_for_level(
        &self,
        level: i32,
        exp_in_level: i64,
    ) -> Option<PlayerProgressionState> {
        let entry = self.entry_for_level(level)?;
        Some(PlayerProgressionState::new(
            level,
            clamp_i64_to_u32(exp_in_level),
            clamp_i64_to_u32(entry.next_exp),
        ))
    }

    pub fn apply_player_exp_gain(
        &self,
        current: PlayerProgressionState,
        amount: i64,
    ) -> Option<PlayerExpGainOutcome> {
        let mut progression =
            self.progression_for_level(current.level, current.exp_in_level.into())?;
        let max_level = self.max_level()?;
        let mut remaining = amount.max(0);
        let mut stat_points_gained = 0;
        let mut level_steps_gained = 0;
        let mut levels_gained = 0;
        let mut applied_exp = 0_i64;

        while remaining > 0 && progression.level < max_level {
            let before_exp = progression.exp_in_level;
            let before_step = progression.level_step();
            let next_exp = progression.next_exp_in_level;
            if next_exp == 0 {
                break;
            }

            let missing = u64::from(next_exp.saturating_sub(before_exp));
            if missing == 0 {
                if !advance_level(&mut progression, self, max_level, &mut levels_gained) {
                    break;
                }
                continue;
            }

            if u64::try_from(remaining).unwrap_or(u64::MAX) >= missing {
                progression.exp_in_level = next_exp;
                applied_exp =
                    applied_exp.saturating_add(i64::try_from(missing).unwrap_or(i64::MAX));
                remaining = remaining.saturating_sub(i64::try_from(missing).unwrap_or(i64::MAX));
                let crossed =
                    crossed_level_steps(progression.level, before_step, LEVEL_STEPS_PER_LEVEL);
                level_steps_gained += crossed.level_steps;
                stat_points_gained += crossed.stat_points;
                if !advance_level(&mut progression, self, max_level, &mut levels_gained) {
                    break;
                }
            } else {
                let gain = remaining as u32;
                progression.exp_in_level = progression.exp_in_level.saturating_add(gain);
                applied_exp = applied_exp.saturating_add(remaining);
                remaining = 0;
                let after_step = progression.level_step();
                let crossed = crossed_level_steps(progression.level, before_step, after_step);
                level_steps_gained += crossed.level_steps;
                stat_points_gained += crossed.stat_points;
            }
        }

        Some(PlayerExpGainOutcome {
            progression,
            applied_exp,
            stat_points_gained,
            level_steps_gained,
            levels_gained,
        })
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum LevelExpTableError {
    #[error("level exp table is empty")]
    Empty,
    #[error("level exp table contains invalid level {level}")]
    InvalidLevel { level: i32 },
    #[error("level {level} has invalid next exp {next_exp}")]
    InvalidNextExp { level: i32, next_exp: i64 },
    #[error("level exp table contains duplicate level {level}")]
    DuplicateLevel { level: i32 },
    #[error("level exp table is missing level {level}")]
    MissingLevel { level: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerExpGainOutcome {
    pub progression: PlayerProgressionState,
    pub applied_exp: i64,
    pub stat_points_gained: i32,
    pub level_steps_gained: i32,
    pub levels_gained: i32,
}

pub fn legacy_mob_exp_reward(
    player_level: i32,
    mob_level: i32,
    base_exp: i64,
    next_exp: i64,
) -> i64 {
    if base_exp <= 0 {
        return 0;
    }

    let adjusted = base_exp
        .saturating_mul(exp_reward_bonus_malus_percent(player_level, mob_level) as i64)
        / 100;

    // cap reward per single kill to 10% of total level-up requirement
    if next_exp > 0 {
        adjusted.min(next_exp / 10).max(0)
    } else {
        adjusted.max(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerMobExpRewardOutcome {
    pub progression: PlayerProgressionState,
    pub applied_exp: i64,
    pub stat_points_gained: i32,
    pub level_steps_gained: i32,
    pub level_up: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerStatRules {
    class_stats: PlayerClassStatsTable,
    level_exp: LevelExpTable,
}

impl PlayerStatRules {
    pub fn new(class_stats: PlayerClassStatsTable, level_exp: LevelExpTable) -> Self {
        Self {
            class_stats,
            level_exp,
        }
    }

    pub fn class_stats(&self) -> &PlayerClassStatsTable {
        &self.class_stats
    }

    pub fn level_exp(&self) -> &LevelExpTable {
        &self.level_exp
    }

    pub fn supports_class(&self, class: DomainPlayerClass) -> bool {
        self.class_stats.stat_source_for_class(class).is_some()
    }

    pub fn resolve_player_stats(
        &self,
        class: DomainPlayerClass,
        allocations: CoreStatAllocations,
    ) -> Option<PlayerStats> {
        self.class_stats.resolve_player_stats(class, allocations)
    }

    pub fn apply_mob_exp_reward(
        &self,
        current: PlayerProgressionState,
        mob_level: i32,
        base_exp: i64,
    ) -> Option<PlayerMobExpRewardOutcome> {
        let amount = legacy_mob_exp_reward(
            current.level,
            mob_level,
            base_exp,
            i64::from(current.next_exp_in_level),
        );
        if amount <= 0 {
            return None;
        }

        let outcome = self.level_exp.apply_player_exp_gain(current, amount)?;
        if outcome.applied_exp <= 0 {
            return None;
        }

        Some(PlayerMobExpRewardOutcome {
            progression: outcome.progression,
            applied_exp: outcome.applied_exp,
            stat_points_gained: outcome.stat_points_gained,
            level_steps_gained: outcome.level_steps_gained,
            level_up: (outcome.progression.level > current.level)
                .then_some(outcome.progression.level),
        })
    }

    pub fn hydrate_player(
        &self,
        bootstrap: &PlayerGameplayBootstrap,
    ) -> Option<HydratedPlayerStats> {
        let stat_source = self.class_stats.stat_source_for_class(bootstrap.class)?;
        let player_stats = self
            .class_stats
            .resolve_player_stats(bootstrap.class, bootstrap.core_stat_allocations)?;
        let progression = self
            .level_exp
            .progression_for_level(bootstrap.level, bootstrap.exp_in_level)?;

        let mut state: ActorStatState = ActorStatState::new(ActorKind::Player);
        let mut api = GameStatsApi::new(&stat_source, &mut state);
        api.set_stable_id(bootstrap.player_id.get() as u64);
        api.set_player_progression(progression);
        api.set_stored_stat(Stat::St, player_stats.stat_str).ok()?;
        api.set_stored_stat(Stat::Ht, player_stats.stat_vit).ok()?;
        api.set_stored_stat(Stat::Dx, player_stats.stat_dex).ok()?;
        api.set_stored_stat(Stat::Iq, player_stats.stat_int).ok()?;
        api.set_stored_stat(
            Stat::StatPoints,
            derive_available_stat_points(
                progression.level,
                progression.level_step(),
                bootstrap.core_stat_allocations,
            ),
        )
        .ok()?;
        api.set_stored_stat(Stat::StatResetCount, bootstrap.stat_reset_count)
            .ok()?;
        api.recompute();

        let current_hp = bootstrap
            .current_hp
            .unwrap_or_else(|| api.read_limited(Stat::MaxHp));
        let current_sp = bootstrap
            .current_sp
            .unwrap_or_else(|| api.read_limited(Stat::MaxSp));
        let current_stamina = bootstrap
            .current_stamina
            .unwrap_or_else(|| api.read_limited(Stat::MaxStamina));
        api.set_resource(Stat::Hp, current_hp).ok()?;
        api.set_resource(Stat::Sp, current_sp).ok()?;
        api.set_resource(Stat::Stamina, current_stamina).ok()?;

        let bootstrap_sync = api.bootstrap_sync();
        Some(HydratedPlayerStats {
            source: stat_source,
            state,
            bootstrap_sync,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydratedPlayerStats {
    pub source: ActorStatSource,
    pub state: ActorStatState,
    pub bootstrap_sync: BootstrapStatsSync,
}

fn derive_available_stat_points(
    level: i32,
    level_step: i32,
    allocations: CoreStatAllocations,
) -> i32 {
    let stat_level = level.clamp(1, MAX_LEVEL_STAT_POINTS_EARNING);
    let current_level_steps = if level < MAX_LEVEL_STAT_POINTS_EARNING {
        level_step.clamp(0, STAT_POINT_STEPS_PER_LEVEL)
    } else {
        0
    };
    let earned = i64::from(stat_level - 1) * i64::from(STAT_POINT_STEPS_PER_LEVEL)
        + i64::from(current_level_steps);
    let spent = i64::from(allocations.allocated_str)
        + i64::from(allocations.allocated_vit)
        + i64::from(allocations.allocated_dex)
        + i64::from(allocations.allocated_int);
    (earned - spent).clamp(0, i64::from(i32::MAX)) as i32
}

fn clamp_i64_to_u32(value: i64) -> u32 {
    value.clamp(0, i64::from(u32::MAX)) as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CrossedLevelSteps {
    level_steps: i32,
    stat_points: i32,
}

fn crossed_level_steps(level: i32, before_step: i32, after_step: i32) -> CrossedLevelSteps {
    let before = before_step.clamp(0, LEVEL_STEPS_PER_LEVEL);
    let after = after_step.clamp(0, LEVEL_STEPS_PER_LEVEL);
    if after <= before {
        return CrossedLevelSteps::default();
    }

    let level_steps = after - before;
    let mut stat_points = 0;
    if level < MAX_LEVEL_STAT_POINTS_EARNING {
        let stat_steps_before = before.min(STAT_POINT_STEPS_PER_LEVEL);
        let stat_steps_after = after.min(STAT_POINT_STEPS_PER_LEVEL);
        stat_points = (stat_steps_after - stat_steps_before).max(0);
    }

    CrossedLevelSteps {
        level_steps,
        stat_points,
    }
}

fn advance_level(
    progression: &mut PlayerProgressionState,
    table: &LevelExpTable,
    max_level: i32,
    levels_gained: &mut i32,
) -> bool {
    if progression.level >= max_level {
        return false;
    }

    let next_level = progression.level.saturating_add(1);
    let Some(next) = table.progression_for_level(next_level, 0) else {
        return false;
    };

    progression.level = next.level;
    progression.exp_in_level = 0;
    progression.next_exp_in_level = next.next_exp_in_level;
    *levels_gained += 1;
    true
}
