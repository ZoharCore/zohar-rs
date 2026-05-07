use super::super::{
    LEVEL_STEPS_PER_LEVEL, LevelExpEntry, LevelExpTable, LevelExpTableError,
    PlayerProgressionState, STAT_POINT_STEPS_PER_LEVEL, legacy_mob_exp_reward,
};

#[test]
fn progression_normalization_clamps_negative_level() {
    assert_eq!(
        PlayerProgressionState::new(-4, 10, 20).normalized(),
        PlayerProgressionState::new(0, 10, 20)
    );
}

#[test]
fn progression_preserves_large_unsigned_thresholds() {
    assert_eq!(
        PlayerProgressionState::new(120, 2_500_000_000, 3_000_000_000).normalized(),
        PlayerProgressionState::new(120, 2_500_000_000, 3_000_000_000)
    );
}

#[test]
fn progression_derives_level_step_from_current_level_exp() {
    assert_eq!(PlayerProgressionState::new(10, 0, 400).level_step(), 0);
    assert_eq!(PlayerProgressionState::new(10, 100, 400).level_step(), 1);
    assert_eq!(PlayerProgressionState::new(10, 200, 400).level_step(), 2);
    assert_eq!(PlayerProgressionState::new(10, 300, 400).level_step(), 3);
    assert_eq!(
        PlayerProgressionState::new(10, 400, 400).level_step(),
        LEVEL_STEPS_PER_LEVEL
    );
}

#[test]
fn exp_gain_grants_visual_steps_stat_points_and_levels_on_full_bar() {
    let table = LevelExpTable::new([
        LevelExpEntry {
            level: 1,
            next_exp: 100,
            death_loss_pct: 5,
        },
        LevelExpEntry {
            level: 2,
            next_exp: 200,
            death_loss_pct: 5,
        },
        LevelExpEntry {
            level: 3,
            next_exp: 300,
            death_loss_pct: 5,
        },
    ]);

    let outcome = table
        .apply_player_exp_gain(PlayerProgressionState::new(1, 20, 100), 95)
        .expect("exp outcome");

    assert_eq!(outcome.progression, PlayerProgressionState::new(2, 15, 200));
    assert_eq!(outcome.applied_exp, 95);
    assert_eq!(outcome.level_steps_gained, LEVEL_STEPS_PER_LEVEL);
    assert_eq!(outcome.stat_points_gained, STAT_POINT_STEPS_PER_LEVEL);
    assert_eq!(outcome.levels_gained, 1);
}

#[test]
fn legacy_mob_exp_reward_applies_level_delta_and_single_gain_cap() {
    assert_eq!(legacy_mob_exp_reward(1, 1, 100, 2_000), 100);
    assert_eq!(legacy_mob_exp_reward(2, 1, 100, 2_000), 100);
    assert_eq!(legacy_mob_exp_reward(7, 1, 100, 2_000), 90);
    assert_eq!(legacy_mob_exp_reward(9, 1, 100, 2_000), 80);
    assert_eq!(legacy_mob_exp_reward(11, 1, 100, 2_000), 50);
    assert_eq!(legacy_mob_exp_reward(16, 1, 100, 2_000), 1);
    assert_eq!(legacy_mob_exp_reward(1, 15, 100, 2_000), 170);
    assert_eq!(legacy_mob_exp_reward(1, 16, 100, 2_000), 180);
    assert_eq!(legacy_mob_exp_reward(1, 16, 100, 500), 50);
}

#[test]
fn level_exp_table_rejects_gaps_and_duplicates() {
    assert_eq!(
        LevelExpTable::try_new([
            LevelExpEntry {
                level: 1,
                next_exp: 100,
                death_loss_pct: 0,
            },
            LevelExpEntry {
                level: 3,
                next_exp: 300,
                death_loss_pct: 0,
            },
        ]),
        Err(LevelExpTableError::MissingLevel { level: 2 })
    );
    assert_eq!(
        LevelExpTable::try_new([
            LevelExpEntry {
                level: 1,
                next_exp: 100,
                death_loss_pct: 0,
            },
            LevelExpEntry {
                level: 1,
                next_exp: 200,
                death_loss_pct: 0,
            },
        ]),
        Err(LevelExpTableError::DuplicateLevel { level: 1 })
    );
}
