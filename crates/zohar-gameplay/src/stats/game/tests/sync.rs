use super::super::{
    ActorImmuneFlags, ActorKind, CharacterAppearance, CharacterUpdate, GameStatsApi,
    PlayerProgressionState, Stat,
};
use super::{TestActorStatState, player_source};

#[test]
fn sync_emits_deltas_character_updates_and_then_goes_quiet() {
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
        sync.character_update,
        Some(CharacterUpdate {
            appearance: CharacterAppearance {
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
    assert_eq!(second_sync.character_update, None);
}

#[test]
fn bootstrap_sync_returns_packet_snapshot_and_appearance() {
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
    assert_eq!(full.character_update.appearance.level, 10);
}

#[test]
fn runtime_only_changes_emit_character_update_without_stat_deltas() {
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
    assert!(sync.character_update.unwrap().immune_flags.stun);
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
