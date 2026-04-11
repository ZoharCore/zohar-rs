use super::super::progression::PlayerProgressionState;

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
fn progression_derives_legacy_level_step_from_current_level_exp() {
    assert_eq!(
        PlayerProgressionState::new(10, 0, 400).quarter_chunks_level_step(),
        0
    );
    assert_eq!(
        PlayerProgressionState::new(10, 100, 400).quarter_chunks_level_step(),
        1
    );
    assert_eq!(
        PlayerProgressionState::new(10, 200, 400).quarter_chunks_level_step(),
        2
    );
    assert_eq!(
        PlayerProgressionState::new(10, 300, 400).quarter_chunks_level_step(),
        3
    );
    assert_eq!(
        PlayerProgressionState::new(10, 400, 400).quarter_chunks_level_step(),
        4
    );
}
