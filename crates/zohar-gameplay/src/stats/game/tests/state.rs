use super::super::stat::{StatContributionKind, StatExt, StatRole};
use super::super::{
    ActorImmuneFlags, ActorKind, ActorResources, ActorStatState, GameStatsApi, Stat, StatWriteError,
};
use super::{TestActorStatState, mob_source, player_source};
use crate::stats::game::store::PointValueStore;

#[test]
fn stat_roles_define_valid_write_and_modifier_channels() {
    assert_eq!(Stat::Level.role(), StatRole::Persistent);
    assert_eq!(Stat::MaxHp.role(), StatRole::Computed);
    assert_eq!(Stat::Hp.role(), StatRole::RuntimeResource);
    assert_eq!(Stat::BonusMaxHp.role(), StatRole::ModifierAccumulator);
    assert_eq!(Stat::Polymorph.role(), StatRole::RuntimeIdentity);
    assert_eq!(Stat::Mount.role(), StatRole::RuntimeIdentity);
    assert_eq!(
        Stat::BonusMaxHp.contribution_kind(),
        Some(StatContributionKind::AdditiveScalar)
    );
    assert_eq!(
        Stat::MaxHpPct.contribution_kind(),
        Some(StatContributionKind::CappedPercentage)
    );
    assert_eq!(
        Stat::ImmuneStun.contribution_kind(),
        Some(StatContributionKind::FlagCounter)
    );
    assert_eq!(Stat::Polymorph.contribution_kind(), None);
    assert_eq!(Stat::Mount.contribution_kind(), None);
    assert!(Stat::BonusMaxHp.accepts_source_contribution());
    assert!(Stat::MaxHpPct.accepts_source_contribution());
    assert!(Stat::ImmuneStun.accepts_source_contribution());
    assert!(Stat::St.accepts_source_contribution());
    assert!(Stat::AttSpeed.accepts_source_contribution());
    assert!(!Stat::Level.accepts_source_contribution());
    assert!(!Stat::MaxHp.accepts_source_contribution());
    assert!(!Stat::DisplayedDefGrade.accepts_source_contribution());
    assert!(!Stat::Polymorph.accepts_source_contribution());
    assert!(!Stat::Mount.accepts_source_contribution());
}

#[test]
fn state_rejects_writes_through_the_wrong_channel() {
    let mut state: ActorStatState = ActorStatState::new(ActorKind::Player);

    assert_eq!(
        state.set_stored_stat(Stat::MaxHp, 500).unwrap_err(),
        StatWriteError::NotStored { stat: Stat::MaxHp }
    );
    assert_eq!(
        state.set_stored_stat(Stat::Level, 10).unwrap_err(),
        StatWriteError::RequiresTypedProgression { stat: Stat::Level }
    );
    assert_eq!(
        state.set_resource_stat(Stat::Ht, 12).unwrap_err(),
        StatWriteError::NotResource { stat: Stat::Ht }
    );
}

#[test]
fn player_core_stat_writes_enforce_player_caps_but_mob_writes_do_not() {
    let player_source = player_source(600, 200, 800, 40, 20, 5);
    let mut player_state = TestActorStatState::new(ActorKind::Player);
    let mut player_api = GameStatsApi::new(&player_source, &mut player_state);

    assert_eq!(
        player_api.set_stored_stat(Stat::Ht, 91).unwrap_err(),
        StatWriteError::OutOfRange {
            stat: Stat::Ht,
            value: 91,
            min: Some(0),
            max: 90,
        }
    );

    let mob_source = mob_source(35, 18, 12, 10, 6, 3_200, 40, 90, 120);
    let mut mob_state = TestActorStatState::new(ActorKind::Mob);
    {
        let mut mob_api = GameStatsApi::new(&mob_source, &mut mob_state);
        mob_api.set_stored_stat(Stat::St, 120).unwrap();
    }
    assert_eq!(mob_state.base().get(Stat::St), 120);
}

#[test]
fn runtime_flags_are_dirty_without_stat_points() {
    let mut state: ActorStatState = ActorStatState::new(ActorKind::Player);
    state.clear_dirty();
    assert!(state.take_runtime_dirty());
    assert!(!state.is_dirty());

    state.set_external_immune_flags(ActorImmuneFlags {
        stun: true,
        slow: false,
        fall: true,
    });

    assert!(state.runtime().external_immune_flags.stun);
    assert!(state.is_dirty());
    assert!(state.take_runtime_dirty());
}

#[test]
fn resources_clamp_to_computed_caps_without_touching_recovery_buckets() {
    let mut resources = ActorResources {
        hp: 500,
        sp: 200,
        stamina: 80,
        hp_recovery: 40,
        sp_recovery: 20,
    };
    let mut caps = PointValueStore::new();
    caps.set(Stat::MaxHp, 100);
    caps.set(Stat::MaxSp, 50);
    caps.set(Stat::MaxStamina, 30);

    resources.clamp_to_caps(&caps);

    assert_eq!(resources.hp, 100);
    assert_eq!(resources.sp, 50);
    assert_eq!(resources.stamina, 30);
    assert_eq!(resources.hp_recovery, 40);
    assert_eq!(resources.sp_recovery, 20);
}
