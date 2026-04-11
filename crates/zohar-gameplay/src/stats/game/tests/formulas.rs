use super::super::{ActorKind, GameStatsApi, PlayerProgressionState, Stat};
use super::{
    TestActorStatState, TestModifierInstance, TestModifierSource, TestModifierSourceKind,
    mob_source, player_source, player_source_with_growth,
};

#[test]
fn player_formula_combines_base_stats_modifiers_and_legacy_bonuses() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::St, 12).unwrap();
    state.set_stored_stat(Stat::Ht, 5).unwrap();
    state.set_stored_stat(Stat::Iq, 7).unwrap();

    let system = TestModifierSource::new(TestModifierSourceKind::System, 1);
    state.modifiers_mut().replace_source(
        system,
        [
            TestModifierInstance::new(system, Stat::AttGradeBonus, 3),
            TestModifierInstance::new(system, Stat::DefGradeBonus, 10),
            TestModifierInstance::new(system, Stat::PartyDefenderBonus, 4),
            TestModifierInstance::new(system, Stat::MagicAttGradeBonus, 5),
            TestModifierInstance::new(system, Stat::MagicDefGradeBonus, 6),
            TestModifierInstance::new(system, Stat::PartyHasteBonus, 7),
        ],
    );

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();

    assert_eq!(api.computed_value(Stat::AttGrade), 47);
    assert_eq!(api.computed_value(Stat::DefGrade), 28);
    assert_eq!(api.computed_value(Stat::DisplayedDefGrade), 29);
    assert_eq!(api.computed_value(Stat::MagicAttGrade), 39);
    assert_eq!(api.computed_value(Stat::MagicDefGrade), 31);
    assert_eq!(api.computed_value(Stat::AttSpeed), 107);
}

#[test]
fn defence_projection_preserves_legacy_client_visible_offset() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();

    let buff = TestModifierSource::new(TestModifierSourceKind::Buff, 42);
    state
        .modifiers_mut()
        .replace_source(buff, [TestModifierInstance::new(buff, Stat::DefGrade, 10)]);

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();

    assert_eq!(api.computed_value(Stat::DefGrade), 24);
    assert_eq!(api.computed_value(Stat::DisplayedDefGrade), 25);
}

#[test]
fn magic_defence_halves_the_combined_legacy_armor_term() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();
    state.set_stored_stat(Stat::Iq, 7).unwrap();

    let system = TestModifierSource::new(TestModifierSourceKind::System, 1);
    state.modifiers_mut().replace_source(
        system,
        [
            TestModifierInstance::new(system, Stat::ArmorDefence, 1),
            TestModifierInstance::new(system, Stat::DefGradeBonus, 1),
        ],
    );

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();

    assert_eq!(api.computed_value(Stat::DefGrade), 16);
    assert_eq!(api.computed_value(Stat::DisplayedDefGrade), 17);
    assert_eq!(api.computed_value(Stat::MagicDefGrade), 19);
}

#[test]
fn stable_id_drives_deterministic_player_growth() {
    let source = player_source_with_growth(600, 200, 800, 40, 20, 5, (36, 40), (12, 16), (5, 8));
    let player = source.player().expect("player source");

    let mut state_a = TestActorStatState::new(ActorKind::Player);
    state_a.set_player_progression(PlayerProgressionState::level_only(10));
    state_a.set_stored_stat(Stat::Ht, 5).unwrap();
    state_a.set_stored_stat(Stat::Iq, 7).unwrap();
    let mut api_a = GameStatsApi::new(&source, &mut state_a);
    api_a.set_stable_id(1001);
    api_a.recompute();

    let expected_hp = 600 + player.growth.random_hp(1001, 10) + (5 * 40);
    let expected_sp = 200 + player.growth.random_sp(1001, 10) + (7 * 20);
    let expected_stamina = 800 + player.growth.random_stamina(1001, 10) + (5 * 5);
    assert_eq!(api_a.computed_value(Stat::MaxHp), expected_hp);
    assert_eq!(api_a.computed_value(Stat::MaxSp), expected_sp);
    assert_eq!(api_a.computed_value(Stat::MaxStamina), expected_stamina);

    let mut state_b = TestActorStatState::new(ActorKind::Player);
    state_b.set_player_progression(PlayerProgressionState::level_only(10));
    state_b.set_stored_stat(Stat::Ht, 5).unwrap();
    state_b.set_stored_stat(Stat::Iq, 7).unwrap();
    let mut api_b = GameStatsApi::new(&source, &mut state_b);
    api_b.set_stable_id(1001);
    api_b.recompute();
    assert_eq!(api_b.computed_value(Stat::MaxHp), expected_hp);

    api_b.set_stable_id(1002);
    api_b.recompute();
    assert_ne!(api_b.computed_value(Stat::MaxHp), expected_hp);
}

#[test]
fn max_resource_pct_stats_cap_and_reduce_derived_resource_caps() {
    let source = player_source(1_000, 500, 800, 0, 0, 0);
    let mut state = TestActorStatState::new(ActorKind::Player);
    let buff = TestModifierSource::new(TestModifierSourceKind::Buff, 89);
    state.modifiers_mut().replace_source(
        buff,
        [
            TestModifierInstance::new(buff, Stat::MaxHpPct, 400),
            TestModifierInstance::new(buff, Stat::MaxSpPct, -20),
        ],
    );

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();

    assert_eq!(api.computed_value(Stat::MaxHpPct), 400);
    assert_eq!(api.computed_value(Stat::MaxSpPct), -20);
    assert_eq!(api.computed_value(Stat::MaxHp), 4_500);
    assert_eq!(api.computed_value(Stat::MaxSp), 400);
}

#[test]
fn mob_formula_uses_proto_baseline_and_core_modifiers() {
    let source = mob_source(35, 18, 12, 10, 6, 3200, 40, 90, 120);
    let mut state = TestActorStatState::new(ActorKind::Mob);
    let buff = TestModifierSource::new(TestModifierSourceKind::Buff, 23);
    state.modifiers_mut().replace_source(
        buff,
        [
            TestModifierInstance::new(buff, Stat::St, 5),
            TestModifierInstance::new(buff, Stat::Ht, 2),
        ],
    );

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();

    assert_eq!(api.computed_value(Stat::MaxHp), 3200);
    assert_eq!(api.computed_value(Stat::St), 23);
    assert_eq!(api.computed_value(Stat::Ht), 14);
    assert_eq!(api.computed_value(Stat::AttGrade), 116);
    assert_eq!(api.computed_value(Stat::DefGrade), 89);
}
