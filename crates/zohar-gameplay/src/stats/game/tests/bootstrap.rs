use super::super::{
    ActorStatSource, CoreStatBlock, DeterministicGrowthVersion, LevelExpEntry, LevelExpTable,
    PlayerClassStatsConfig, PlayerClassStatsTable, PlayerGrowthFormula, PlayerResourceFormula,
    PlayerStatRules, PlayerStatSource, SourceSpeeds, Stat, default_player_balance_rules,
};
use zohar_domain::entity::player::{CoreStatAllocations, PlayerClass, PlayerGameplayBootstrap};

fn warrior_config() -> PlayerClassStatsConfig {
    PlayerClassStatsConfig {
        base_stats: CoreStatBlock::new(6, 4, 3, 3),
        stat_source: ActorStatSource::Player(PlayerStatSource {
            resources: PlayerResourceFormula {
                base_max_hp: 600,
                base_max_sp: 200,
                base_max_stamina: 800,
                hp_per_ht: 40,
                sp_per_iq: 20,
                stamina_per_ht: 5,
            },
            growth: PlayerGrowthFormula {
                hp_per_level: (36, 44),
                sp_per_level: (18, 22),
                stamina_per_level: (5, 8),
                version: DeterministicGrowthVersion::V1,
            },
            balance: default_player_balance_rules(PlayerClass::Warrior),
            speeds: SourceSpeeds::default(),
        }),
    }
}

#[test]
fn player_class_stats_table_resolves_absolute_stats() {
    let table = PlayerClassStatsTable::new(vec![(PlayerClass::Warrior, warrior_config())]);
    let stats = table
        .resolve_player_stats(
            PlayerClass::Warrior,
            CoreStatAllocations {
                allocated_str: 2,
                allocated_vit: 1,
                allocated_dex: 0,
                allocated_int: 3,
            },
        )
        .expect("resolved stats");

    assert_eq!(stats.stat_str, 8);
    assert_eq!(stats.stat_vit, 5);
    assert_eq!(stats.stat_dex, 3);
    assert_eq!(stats.stat_int, 6);
}

#[test]
fn level_exp_table_builds_progression_snapshots_and_max_level() {
    let table = LevelExpTable::new([
        LevelExpEntry {
            level: 1,
            next_exp: 300,
            death_loss_pct: 5,
        },
        LevelExpEntry {
            level: 10,
            next_exp: 33_000,
            death_loss_pct: 4,
        },
        LevelExpEntry {
            level: 120,
            next_exp: 2_500_000_000,
            death_loss_pct: 1,
        },
    ]);

    let progression = table
        .progression_for_level(10, 12_345)
        .expect("progression");

    assert_eq!(progression.level, 10);
    assert_eq!(progression.exp_in_level, 12_345);
    assert_eq!(progression.next_exp_in_level, 33_000);
    assert_eq!(table.max_level(), Some(120));
}

#[test]
fn player_stat_rules_hydrate_bootstrap_and_packet_projection() {
    let rules = PlayerStatRules::new(
        PlayerClassStatsTable::new(vec![(PlayerClass::Warrior, warrior_config())]),
        LevelExpTable::new([LevelExpEntry {
            level: 10,
            next_exp: 33_000,
            death_loss_pct: 4,
        }]),
    );

    let hydrated = rules
        .hydrate_player(&PlayerGameplayBootstrap {
            player_id: 7_i64.into(),
            class: PlayerClass::Warrior,
            level: 10,
            exp_in_level: 12_345,
            core_stat_allocations: CoreStatAllocations {
                allocated_str: 2,
                allocated_vit: 1,
                allocated_dex: 0,
                allocated_int: 3,
            },
            stat_reset_count: 0,
            current_hp: None,
            current_sp: None,
            current_stamina: None,
        })
        .expect("hydrated stats");

    assert_eq!(hydrated.bootstrap_sync.stat_snapshot.get(Stat::St), 8);
    assert_eq!(
        hydrated.bootstrap_sync.character_update.appearance.level,
        10
    );
}
