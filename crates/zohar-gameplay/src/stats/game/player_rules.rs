use std::collections::BTreeMap;

use zohar_domain::entity::player::{
    CoreStatAllocations, PlayerClass as DomainPlayerClass, PlayerGameplayBootstrap, PlayerStats,
};

use super::{
    ActorKind, ActorStatSource, ActorStatState, BootstrapStatsSync, CoreStatBlock, GameStatsApi,
    PlayerProgressionState, Stat,
};

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
        Self(
            entries
                .into_iter()
                .map(|entry| (entry.level, entry))
                .collect(),
        )
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
                progression.quarter_chunks_level_step(),
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
    let earned = i64::from(level.max(1) - 1) * 3 + i64::from(level_step.max(0));
    let spent = i64::from(allocations.allocated_str)
        + i64::from(allocations.allocated_vit)
        + i64::from(allocations.allocated_dex)
        + i64::from(allocations.allocated_int);
    (earned - spent).clamp(0, i64::from(i32::MAX)) as i32
}

fn clamp_i64_to_u32(value: i64) -> u32 {
    value.clamp(0, i64::from(u32::MAX)) as u32
}
