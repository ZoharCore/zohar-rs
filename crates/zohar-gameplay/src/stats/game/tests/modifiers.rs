use super::super::{
    ActorKind, CompiledModifier, CompiledStatContribution, GameStatsApi, PlayerProgressionState,
    SourceBundleError, Stat,
};
use super::{
    TestActorStatState, TestModifierDetail, TestModifierInstance, TestModifierLedger,
    TestModifierSource, TestModifierSourceKind, player_source,
};

#[test]
fn game_modifier_ledger_replaces_all_entries_for_a_source() {
    let source = TestModifierSource::new(TestModifierSourceKind::EquipmentSlot, 2);
    let mut ledger = TestModifierLedger::default();
    ledger.replace_source(
        source,
        [
            TestModifierInstance::new(source, Stat::BonusMaxHp, 11).equipment_apply(1),
            TestModifierInstance::new(source, Stat::BonusMaxSp, 24).equipment_apply(2),
            TestModifierInstance::new(source, Stat::Iq, 3).equipment_apply(0),
        ],
    );

    ledger.replace_source(
        source,
        [TestModifierInstance::new(source, Stat::Iq, 5).equipment_apply(0)],
    );

    assert_eq!(ledger.total_for_stat(Stat::BonusMaxHp), 0);
    assert_eq!(ledger.total_for_stat(Stat::BonusMaxSp), 0);
    assert_eq!(ledger.total_for_stat(Stat::Iq), 5);
}

#[test]
fn compiled_contributions_replace_all_source_owned_channels() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    let buff = TestModifierSource::new(TestModifierSourceKind::Buff, 99);
    api.replace_source_bundle(
        buff,
        CompiledStatContribution::new().with_modifier(CompiledModifier::new(
            Stat::BonusMaxHp,
            75,
            TestModifierDetail::None,
        )),
    )
    .unwrap();

    let sync = api.sync();

    assert!(sync.changes.contains(Stat::MaxHp));
    assert_eq!(api.computed_value(Stat::MaxHp), 875);

    api.remove_source_bundle(buff);
    let sync = api.sync();

    assert!(sync.changes.contains(Stat::MaxHp));
    assert_eq!(api.computed_value(Stat::MaxHp), 800);
}

#[test]
fn source_bundle_rejects_recovery_stats_in_modifier_channel() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    let mut api = GameStatsApi::new(&source, &mut state);
    let buff = TestModifierSource::new(TestModifierSourceKind::Buff, 10);

    let error = api
        .replace_source_bundle(
            buff,
            CompiledStatContribution::new().with_modifier(CompiledModifier::new(
                Stat::HpRecovery,
                5,
                TestModifierDetail::None,
            )),
        )
        .unwrap_err();

    assert_eq!(
        error,
        SourceBundleError::RecoveryStatInModifierChannel {
            stat: Stat::HpRecovery,
        }
    );
}
