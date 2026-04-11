use super::super::{ActorKind, GameStatsApi, PlayerProgressionState, ResourceApplication, Stat};
use super::{TestActorStatState, player_source};

#[test]
fn resource_application_can_clamp_nonlethal_damage() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();
    state.set_stored_stat(Stat::Iq, 7).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    api.set_resource(Stat::Hp, 25).unwrap();

    let result = api.apply_resource(ResourceApplication::poison(40)).unwrap();

    assert_eq!(result.previous, 25);
    assert_eq!(result.current, 1);
    assert_eq!(result.applied_delta, -24);
    assert!(result.was_clamped);
}

#[test]
fn pending_recovery_application_consumes_caller_scheduled_amount() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    api.set_resource(Stat::Hp, 25).unwrap();
    api.queue_recovery(Stat::Hp, 100).unwrap().unwrap();

    let result = api.apply_pending_recovery(Stat::Hp, 56).unwrap();

    assert_eq!(result.stat, Stat::Hp);
    assert_eq!(result.previous, 25);
    assert_eq!(result.current, 81);
    assert_eq!(api.read_packet(Stat::HpRecovery), 44);
}

#[test]
fn pending_recovery_application_clears_bucket_when_resource_is_full() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    let max_hp = api.read_limited(Stat::MaxHp);
    api.queue_recovery(Stat::Hp, 10).unwrap().unwrap();
    api.set_resource(Stat::Hp, max_hp).unwrap();

    let result = api.apply_pending_recovery(Stat::Hp, 56).unwrap();

    assert_eq!(result.applied_delta, 0);
    assert_eq!(api.read_packet(Stat::HpRecovery), 0);
}

#[test]
fn fixed_and_auto_recovery_queue_only_missing_amounts() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    api.set_resource(Stat::Hp, 790).unwrap();

    let queued = api.queue_recovery(Stat::Hp, 100).unwrap().unwrap();
    assert_eq!(queued.queued_amount, 100);
    assert_eq!(api.read_packet(Stat::HpRecovery), 100);
    assert!(api.queue_recovery(Stat::Hp, 100).unwrap().is_none());

    api.apply_pending_recovery(Stat::Hp, 100).unwrap();
    api.set_resource(Stat::Hp, 790).unwrap();
    let queued = api.queue_auto_recovery(Stat::Hp, 100).unwrap().unwrap();
    assert_eq!(queued.queued_amount, 10);
    assert_eq!(api.read_packet(Stat::HpRecovery), 10);
    assert!(api.queue_auto_recovery(Stat::Hp, 100).unwrap().is_none());
}
