use super::super::{
    ActorImmuneFlags, ActorKind, ActorPublicState, ActorPublicStats, ActorStatState, GameStatsApi,
    PlayerProgressionState, PlayerStatsRuntime, Stat,
};
use super::{TestActorStatState, player_source};

#[test]
fn sync_emits_deltas_public_actor_state_and_then_goes_quiet() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();
    state.set_stored_stat(Stat::Iq, 7).unwrap();
    state.set_resource_stat(Stat::Hp, 9_999).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    let sync = api.sync();

    assert!(sync.changes.contains(Stat::MaxHp));
    assert!(sync.changes.contains(Stat::Hp));
    assert!(
        sync.stat_deltas
            .iter()
            .any(|delta| delta.stat == Stat::MaxHp)
    );
    assert!(sync.stat_deltas.iter().any(|delta| delta.stat == Stat::Hp));
    assert_eq!(
        sync.public_state,
        Some(ActorPublicState {
            stats: ActorPublicStats {
                level: 10,
                move_speed: 100,
                attack_speed: 100,
            },
            immune_flags: ActorImmuneFlags::default(),
        })
    );

    let second_sync = api.sync();
    assert!(second_sync.changes.is_empty());
    assert!(second_sync.stat_deltas.is_empty());
    assert_eq!(second_sync.public_state, None);
}

#[test]
fn bootstrap_sync_returns_stat_snapshot_and_public_actor_state() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));
    state.set_stored_stat(Stat::Ht, 5).unwrap();
    state.set_stored_stat(Stat::Iq, 7).unwrap();
    state.set_resource_stat(Stat::Hp, 9_999).unwrap();

    let mut api = GameStatsApi::new(&source, &mut state);
    let full = api.bootstrap_sync();

    assert!(full.changes.contains(Stat::MaxHp));
    assert_eq!(full.stat_snapshot.get(Stat::MaxHp), 800);
    assert_eq!(full.stat_snapshot.get(Stat::Hp), 800);
    assert_eq!(full.public_state.stats.level, 10);
}

#[test]
fn runtime_only_changes_emit_public_state_without_stat_deltas() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::level_only(10));

    let mut api = GameStatsApi::new(&source, &mut state);
    api.sync();
    api.set_external_immune_flags(ActorImmuneFlags {
        stun: true,
        slow: true,
        fall: false,
    });

    let sync = api.sync_if_dirty();

    assert!(sync.changes.is_empty());
    assert!(sync.stat_deltas.is_empty());
    assert!(sync.public_state.unwrap().immune_flags.stun);
}

#[test]
fn recompute_consumes_dirty_state() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let mut state = TestActorStatState::new(ActorKind::Player);

    let mut api = GameStatsApi::new(&source, &mut state);
    api.recompute();
    assert!(!api.is_dirty());

    api.set_external_immune_flags(ActorImmuneFlags {
        stun: true,
        slow: false,
        fall: false,
    });
    assert!(api.is_dirty());

    api.recompute();
    assert!(!api.is_dirty());
}

#[test]
fn player_stats_runtime_coalesces_multiple_normalizations_before_drain() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let state = ActorStatState::new(ActorKind::Player);
    let mut runtime = PlayerStatsRuntime::new(source, state);

    runtime
        .with_api_mut(|api| api.set_stored_stat(Stat::Ht, 5))
        .unwrap();
    runtime.normalize();
    runtime
        .with_api_mut(|api| api.set_resource(Stat::Hp, 100))
        .unwrap();
    runtime.normalize();
    runtime
        .with_api_mut(|api| api.set_resource(Stat::Hp, 125))
        .unwrap();

    let drained = runtime.drain_sync();

    assert_eq!(
        drained
            .stat_deltas
            .iter()
            .filter(|delta| delta.stat == Stat::Hp)
            .count(),
        1
    );
    assert!(
        drained
            .stat_deltas
            .iter()
            .any(|delta| delta.stat == Stat::Hp && delta.value == 125)
    );
}

#[test]
fn player_stats_runtime_drain_normalizes_dirty_state() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let state = ActorStatState::new(ActorKind::Player);
    let mut runtime = PlayerStatsRuntime::new(source, state);

    runtime
        .with_api_mut(|api| api.set_stored_stat(Stat::Ht, 5))
        .unwrap();

    let drained = runtime.drain_sync();

    assert!(
        drained
            .stat_deltas
            .iter()
            .any(|delta| delta.stat == Stat::Ht)
    );
    assert!(
        drained
            .stat_deltas
            .iter()
            .any(|delta| delta.stat == Stat::MaxHp)
    );
}

#[test]
fn player_stats_runtime_preserves_pending_zero_values() {
    let source = player_source(600, 200, 800, 40, 20, 5);
    let state = ActorStatState::new(ActorKind::Player);
    let mut runtime = PlayerStatsRuntime::new(source, state);

    runtime
        .with_api_mut(|api| api.set_stored_stat(Stat::Gold, 10))
        .unwrap();
    runtime.normalize();
    runtime
        .with_api_mut(|api| api.set_stored_stat(Stat::Gold, 0))
        .unwrap();

    let drained = runtime.drain_sync();

    assert!(
        drained
            .stat_deltas
            .iter()
            .any(|delta| delta.stat == Stat::Gold && delta.value == 0),
        "expected explicit zero-valued pending stat, got {:?}",
        drained.stat_deltas
    );
}
