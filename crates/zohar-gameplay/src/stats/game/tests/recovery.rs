use std::time::Duration;

use super::super::{
    ActorKind, GameStatsApi, PlayerPassiveHpRecoveryState, PlayerPassiveSpRecoveryState,
    PlayerProgressionState, Stat, tick_player_passive_hp_recovery, tick_player_passive_sp_recovery,
};
use super::{TestActorStatState, player_source};

#[test]
fn passive_recovery_stats_applies_and_reports_changed_resources() {
    let source = player_source(1_000, 300, 800, 0, 0, 0);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    api.set_resource(Stat::Hp, 500).unwrap();
    api.set_resource(Stat::Sp, 100).unwrap();

    let mut hp_recovery = PlayerPassiveHpRecoveryState::default();
    let mut sp_recovery = PlayerPassiveSpRecoveryState::default();
    let hp_application = tick_player_passive_hp_recovery(
        &mut hp_recovery,
        api.read_packet(Stat::Hp),
        api.read_limited(Stat::MaxHp),
        Default::default(),
        Duration::from_secs(3),
    )
    .unwrap();
    let hp = api.apply_resource(hp_application).unwrap();

    let sp_application = tick_player_passive_sp_recovery(
        &mut sp_recovery,
        api.read_packet(Stat::Hp),
        api.read_limited(Stat::MaxHp),
        api.read_packet(Stat::Sp),
        api.read_limited(Stat::MaxSp),
        Default::default(),
        Default::default(),
        Duration::from_secs(3),
    )
    .unwrap();
    let sp = api.apply_resource(sp_application).unwrap();

    assert_eq!(hp.applied_delta, 65);
    assert_eq!(hp.current, 565);
    assert_eq!(sp.applied_delta, 5);
    assert_eq!(sp.current, 105);
}
