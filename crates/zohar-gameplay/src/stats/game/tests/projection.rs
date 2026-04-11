use super::super::view::{StatValueView, read_stat_value};
use super::super::{ActorImmuneFlags, ActorKind, ActorStatState, PlayerProgressionState, Stat};

#[test]
fn limited_view_clamps_speeds_by_actor_kind() {
    let mut player: ActorStatState = ActorStatState::new(ActorKind::Player);
    player.overwrite_computed_from_base();
    player.set_computed_stat(Stat::MovSpeed, 260);
    player.set_computed_stat(Stat::AttSpeed, -5);
    let player_source = super::player_source(0, 0, 0, 0, 0, 0);

    assert_eq!(
        read_stat_value(
            &player,
            Some(&player_source),
            Stat::MovSpeed,
            StatValueView::Limited
        ),
        200
    );
    assert_eq!(
        read_stat_value(
            &player,
            Some(&player_source),
            Stat::AttSpeed,
            StatValueView::Limited
        ),
        0
    );

    let mut mob: ActorStatState = ActorStatState::new(ActorKind::Mob);
    mob.overwrite_computed_from_base();
    mob.set_computed_stat(Stat::MovSpeed, 260);
    let mob_source = super::mob_source(1, 0, 0, 0, 0, 1, 0, 0, 0);

    assert_eq!(
        read_stat_value(
            &mob,
            Some(&mob_source),
            Stat::MovSpeed,
            StatValueView::Limited
        ),
        250
    );
}

#[test]
fn packet_view_prefers_typed_channels_for_legacy_wire_values() {
    let mut state: ActorStatState = ActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::new(12, 123, 400));
    state.set_stored_stat(Stat::Gold, 456).unwrap();
    state.set_stored_stat(Stat::St, 4).unwrap();
    state.overwrite_computed_from_base();
    state.set_computed_stat(Stat::Exp, 999);
    state.set_computed_stat(Stat::Gold, 999);
    state.set_computed_stat(Stat::St, 11);
    state.set_resource_stat(Stat::Hp, 321).unwrap();
    state.set_resource_stat(Stat::Sp, 22).unwrap();
    state.set_resource_stat(Stat::Stamina, 11).unwrap();
    state.set_resource_stat(Stat::HpRecovery, 12).unwrap();
    state.set_external_immune_flags(ActorImmuneFlags {
        stun: true,
        slow: false,
        fall: false,
    });

    assert_eq!(
        read_stat_value(&state, None, Stat::Exp, StatValueView::WireCompatible),
        123
    );
    assert_eq!(
        read_stat_value(&state, None, Stat::Gold, StatValueView::WireCompatible),
        456
    );
    assert_eq!(
        read_stat_value(&state, None, Stat::Hp, StatValueView::WireCompatible),
        321
    );
    assert_eq!(
        read_stat_value(&state, None, Stat::St, StatValueView::WireCompatible),
        11
    );
    assert_eq!(
        read_stat_value(
            &state,
            None,
            Stat::HpRecovery,
            StatValueView::WireCompatible
        ),
        12
    );
    assert_eq!(
        read_stat_value(&state, None, Stat::Mount, StatValueView::WireCompatible),
        0
    );
}

#[test]
fn packet_view_clamps_large_progression_values_for_i32_readers() {
    let mut state: ActorStatState = ActorStatState::new(ActorKind::Player);
    state.set_player_progression(PlayerProgressionState::new(
        120,
        2_500_000_000,
        3_000_000_000,
    ));

    assert_eq!(
        read_stat_value(&state, None, Stat::Exp, StatValueView::WireCompatible),
        i32::MAX
    );
    assert_eq!(
        read_stat_value(&state, None, Stat::NextExp, StatValueView::WireCompatible),
        i32::MAX
    );
}
