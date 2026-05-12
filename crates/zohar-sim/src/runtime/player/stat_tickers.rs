use bevy::prelude::*;
use tracing::warn;
use zohar_domain::entity::MovementAnimation;
use zohar_domain::entity::player::PlayerClass;
use zohar_domain::entity::player::PlayerId;
use zohar_gameplay::stats::game::{
    PlayerMovementActivity, PlayerPassiveHpRecoveryState, PlayerPassiveSpRecoveryState,
    PlayerSpRecoveryProfile, PlayerStaminaMovementOverride, PlayerStaminaState, PlayerStatActivity,
    Stat, tick_player_passive_hp_recovery, tick_player_passive_sp_recovery, tick_player_stamina,
};

use crate::runtime::player::{PlayerMotion, PlayerTicker};
use crate::runtime::state::ActorLifeComp;
use crate::runtime::time::SimTickerClock;

use super::super::actor_stats::apply_actor_resource;
use super::super::facts::{ActorRef, FrameFacts, PlayerStaminaTimerChanged};
use super::super::state::{
    NetEntityId, PlayerActivityComp, PlayerMovementAnimation, PlayerProgressionComp,
    PlayerStatTickerComp, PlayerStatsComp, RuntimeState, SimDuration, SimInstant,
};

impl<T: Default> PlayerTicker<T> {
    fn initial(player_id: PlayerId, now: SimInstant, cadence: SimDuration) -> Self {
        Self {
            clock: SimTickerClock::phased(i64::from(player_id), now, cadence),
            state: T::default(),
        }
    }
}

impl PlayerStatTickerComp {
    const PASSIVE_HP_CADENCE: SimDuration = SimDuration::from_millis(250);
    const PASSIVE_SP_CADENCE: SimDuration = SimDuration::from_millis(250);
    const LEGACY_STAMINA_CADENCE: SimDuration = SimDuration::from_millis(250);

    pub(crate) fn initial(player_id: PlayerId, now: SimInstant) -> Self {
        Self {
            passive_hp: PlayerTicker::initial(player_id, now, Self::PASSIVE_HP_CADENCE),
            passive_sp: PlayerTicker::initial(player_id, now, Self::PASSIVE_SP_CADENCE),
            stamina: PlayerTicker::initial(player_id, now, Self::LEGACY_STAMINA_CADENCE),
        }
    }

    pub(crate) fn reset_after_restart(&mut self, now: SimInstant) {
        self.reset_after_regeneration_pause(now);
    }

    fn reset_after_regeneration_pause(&mut self, now: SimInstant) {
        self.passive_hp
            .clock
            .retry_after(now, Self::PASSIVE_HP_CADENCE);
        self.passive_hp.state = PlayerPassiveHpRecoveryState::default();

        self.passive_sp
            .clock
            .retry_after(now, Self::PASSIVE_SP_CADENCE);
        self.passive_sp.state = PlayerPassiveSpRecoveryState::default();

        self.stamina
            .clock
            .retry_after(now, Self::LEGACY_STAMINA_CADENCE);
        self.stamina.state = PlayerStaminaState::default();
    }
}

impl Default for PlayerTicker<PlayerPassiveHpRecoveryState> {
    fn default() -> Self {
        PlayerTicker::initial(
            PlayerId::from(0),
            SimInstant::ZERO,
            PlayerStatTickerComp::PASSIVE_HP_CADENCE,
        )
    }
}

impl Default for PlayerTicker<PlayerPassiveSpRecoveryState> {
    fn default() -> Self {
        PlayerTicker::initial(
            PlayerId::from(0),
            SimInstant::ZERO,
            PlayerStatTickerComp::PASSIVE_SP_CADENCE,
        )
    }
}

impl Default for PlayerTicker<PlayerStaminaState> {
    fn default() -> Self {
        PlayerTicker::initial(
            PlayerId::from(0),
            SimInstant::ZERO,
            PlayerStatTickerComp::LEGACY_STAMINA_CADENCE,
        )
    }
}

pub(crate) fn process_player_stat_tickers(world: &mut World) {
    let now = world.resource::<RuntimeState>().sim_now;
    let players = super::players::player_entities_on_map(world);

    for player_entity in players {
        if !world.entities().contains(player_entity) {
            continue;
        }
        if !player_can_regenerate(world, player_entity) {
            pause_player_stat_tickers(world, player_entity, now);
            continue;
        }

        let Some(activity) = current_player_stat_activity(world, player_entity, now) else {
            continue;
        };
        tick_passive_hp(world, player_entity, activity, now);
        tick_passive_sp(world, player_entity, activity, now);
        tick_stamina(world, player_entity, activity, now);
    }
}

fn pause_player_stat_tickers(world: &mut World, player_entity: Entity, now: SimInstant) {
    let mut entity = world.entity_mut(player_entity);
    let Some(mut tickers) = entity.get_mut::<PlayerStatTickerComp>() else {
        return;
    };
    tickers.reset_after_regeneration_pause(now);
}

fn player_stat_activity(
    now: SimInstant,
    movement_end_at: Option<SimInstant>,
    activity: &PlayerActivityComp,
) -> PlayerStatActivity {
    PlayerStatActivity {
        movement: match movement_end_at {
            Some(end_at) if end_at > now => PlayerMovementActivity::Moving,
            Some(end_at) => PlayerMovementActivity::stopped_for(
                now.elapsed_since(Some(end_at))
                    .unwrap_or(std::time::Duration::ZERO),
            ),
            None => PlayerMovementActivity::default(),
        },
        since_attack: now.elapsed_since(activity.last_attack_at),
        since_walk_started: now.elapsed_since(activity.last_walk_started_at),
    }
}

fn player_can_regenerate(world: &World, player_entity: Entity) -> bool {
    world
        .entity(player_entity)
        .get::<ActorLifeComp>()
        .is_none_or(ActorLifeComp::can_regenerate)
}

fn current_player_stat_activity(
    world: &World,
    player_entity: Entity,
    now: SimInstant,
) -> Option<PlayerStatActivity> {
    let activity = world.entity(player_entity).get::<PlayerActivityComp>()?;
    let movement_end_at = world
        .entity(player_entity)
        .get::<PlayerMotion>()
        .and_then(|motion_comp| player_movement_end_at(motion_comp, activity));
    Some(player_stat_activity(now, movement_end_at, activity))
}

fn player_movement_end_at(
    motion_comp: &PlayerMotion,
    activity: &PlayerActivityComp,
) -> Option<SimInstant> {
    let movement_started_at = activity.last_movement_start_at?;
    let duration = motion_comp
        .0
        .segment_end_ts
        .saturating_sub(motion_comp.0.segment_start_ts);
    Some(movement_started_at.saturating_add(SimDuration::from_packet_duration(duration)))
}

fn tick_passive_hp(
    world: &mut World,
    player_entity: Entity,
    activity: PlayerStatActivity,
    now: SimInstant,
) {
    let Some((hp, max_hp)) = read_player_stats(world, player_entity, |stats| {
        (
            stats.0.read_packet(Stat::Hp),
            stats.0.read_limited(Stat::MaxHp),
        )
    }) else {
        return;
    };

    let application = {
        let mut entity = world.entity_mut(player_entity);
        let Some(mut tickers) = entity.get_mut::<PlayerStatTickerComp>() else {
            return;
        };
        tickers
            .passive_hp
            .clock
            .advance_due(now)
            .and_then(|elapsed| {
                tick_player_passive_hp_recovery(
                    &mut tickers.passive_hp.state,
                    hp,
                    max_hp,
                    activity,
                    elapsed.as_duration(),
                )
            })
    };

    apply_player_resource_application(world, player_entity, application);
}

fn tick_passive_sp(
    world: &mut World,
    player_entity: Entity,
    activity: PlayerStatActivity,
    now: SimInstant,
) {
    let Some((hp, max_hp, sp, max_sp, profile)) =
        read_player_stats(world, player_entity, |stats| {
            let profile = world
                .entity(player_entity)
                .get::<PlayerProgressionComp>()
                .map(|progression| sp_recovery_profile(progression.0.class))?;
            Some((
                stats.0.read_packet(Stat::Hp),
                stats.0.read_limited(Stat::MaxHp),
                stats.0.read_packet(Stat::Sp),
                stats.0.read_limited(Stat::MaxSp),
                profile,
            ))
        })
        .flatten()
    else {
        return;
    };

    let application = {
        let mut entity = world.entity_mut(player_entity);
        let Some(mut tickers) = entity.get_mut::<PlayerStatTickerComp>() else {
            return;
        };
        tickers
            .passive_sp
            .clock
            .advance_due(now)
            .and_then(|elapsed| {
                tick_player_passive_sp_recovery(
                    &mut tickers.passive_sp.state,
                    hp,
                    max_hp,
                    sp,
                    max_sp,
                    activity,
                    profile,
                    elapsed.as_duration(),
                )
            })
    };

    apply_player_resource_application(world, player_entity, application);
}

fn tick_stamina(
    world: &mut World,
    player_entity: Entity,
    activity: PlayerStatActivity,
    now: SimInstant,
) {
    let Some((stamina, max_stamina, movement_mode)) =
        read_player_stats(world, player_entity, |stats| {
            let movement_mode = world
                .entity(player_entity)
                .get::<PlayerMovementAnimation>()
                .map(|animation| animation.0)?;
            Some((
                stats.0.read_packet(Stat::Stamina),
                stats.0.read_limited(Stat::MaxStamina),
                movement_mode,
            ))
        })
        .flatten()
    else {
        return;
    };

    let stamina_effect = {
        let mut entity = world.entity_mut(player_entity);
        let Some(mut tickers) = entity.get_mut::<PlayerStatTickerComp>() else {
            return;
        };
        tickers.stamina.clock.advance_due(now).map(|elapsed| {
            tick_player_stamina(
                &mut tickers.stamina.state,
                stamina,
                max_stamina,
                activity,
                movement_mode,
                elapsed.as_duration(),
            )
        })
    };
    let Some(stamina_effect) = stamina_effect else {
        return;
    };

    let current_stamina =
        apply_player_resource_application(world, player_entity, stamina_effect.application)
            .unwrap_or_else(|| read_player_stat(world, player_entity, Stat::Stamina).unwrap_or(0));

    if let Some(command) = stamina_effect.timer
        && let Some(player) = actor_ref(world, player_entity)
    {
        world
            .resource_mut::<FrameFacts>()
            .projection
            .stamina_timer_changed
            .push(PlayerStaminaTimerChanged {
                player,
                command,
                current_stamina,
            });
    }

    if let Some(override_mode) = stamina_effect.movement_override {
        let mut query = world.query::<(&mut PlayerMovementAnimation, &mut PlayerActivityComp)>();
        let Ok((mut movement_animation, mut activity)) = query.get_mut(world, player_entity) else {
            return;
        };
        let _ = ascertain_movement_animation_change(
            now,
            &mut movement_animation,
            &mut activity,
            override_mode,
        );
    }
}

fn apply_player_resource_application(
    world: &mut World,
    player_entity: Entity,
    application: Option<zohar_gameplay::stats::game::ResourceApplication>,
) -> Option<i32> {
    let application = application?;
    let actor = actor_ref(world, player_entity)?;
    match apply_actor_resource(world, actor, application) {
        Ok(Some(result)) => Some(result.current),
        Ok(None) => None,
        Err(error) => {
            warn!(
                error = %error,
                "Failed to apply player stat ticker"
            );
            debug_assert!(
                false,
                "player stat ticker produced invalid resource application: {error}"
            );
            None
        }
    }
}

fn read_player_stats<T>(
    world: &World,
    player_entity: Entity,
    read: impl FnOnce(&PlayerStatsComp) -> T,
) -> Option<T> {
    world
        .entity(player_entity)
        .get::<PlayerStatsComp>()
        .map(read)
}

fn read_player_stat(world: &World, player_entity: Entity, stat: Stat) -> Option<i32> {
    read_player_stats(world, player_entity, |stats| stats.0.read_packet(stat))
}

fn actor_ref(world: &World, player_entity: Entity) -> Option<ActorRef> {
    let net_id = world.entity(player_entity).get::<NetEntityId>()?.net_id;
    Some(ActorRef::new(player_entity, net_id))
}

fn ascertain_movement_animation_change(
    now: SimInstant,
    movement_animation: &mut PlayerMovementAnimation,
    activity: &mut PlayerActivityComp,
    override_mode: PlayerStaminaMovementOverride,
) -> bool {
    if override_mode == PlayerStaminaMovementOverride::ForceWalk {
        activity.last_walk_started_at = Some(now);
    }

    let new_animation = match override_mode {
        PlayerStaminaMovementOverride::ForceWalk => MovementAnimation::Walk,
        PlayerStaminaMovementOverride::RevertToPreferred => activity.preferred_movement_animation,
    };

    if movement_animation.0 != new_animation {
        movement_animation.0 = new_animation;
        true
    } else {
        false
    }
}

fn sp_recovery_profile(class: PlayerClass) -> PlayerSpRecoveryProfile {
    match class {
        PlayerClass::Warrior | PlayerClass::Ninja | PlayerClass::Sura => {
            PlayerSpRecoveryProfile::Standard
        }
        // TODO: Sura with Black Magic skill tree is also a caster
        PlayerClass::Shaman => PlayerSpRecoveryProfile::Caster,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::coords::LocalPos;
    use zohar_map_port::ClientTimestamp;

    #[test]
    fn active_motion_has_no_finished_movement_age() {
        let activity = player_stat_activity(
            SimInstant::from_millis(1_000),
            Some(SimInstant::from_millis(2_000)),
            &PlayerActivityComp::default(),
        );

        assert_eq!(activity.movement, PlayerMovementActivity::Moving);
    }

    #[test]
    fn movement_activity_uses_server_start_plus_packet_duration() {
        let motion = PlayerMotion(crate::runtime::state::PlayerMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(4.0, 1.0),
            segment_start_ts: ClientTimestamp::new(10_000),
            segment_end_ts: ClientTimestamp::new(12_000),
            last_client_ts: ClientTimestamp::new(10_000),
        });
        let activity = PlayerActivityComp {
            last_movement_start_at: Some(SimInstant::from_millis(3_600_000)),
            ..Default::default()
        };

        assert_eq!(
            player_movement_end_at(&motion, &activity),
            Some(SimInstant::from_millis(3_602_000))
        );
        assert_eq!(
            player_stat_activity(
                SimInstant::from_millis(3_600_500),
                player_movement_end_at(&motion, &activity),
                &activity,
            )
            .movement,
            PlayerMovementActivity::Moving
        );
    }

    #[test]
    fn zero_server_start_can_still_represent_active_motion() {
        let motion = PlayerMotion(crate::runtime::state::PlayerMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(4.0, 1.0),
            segment_start_ts: ClientTimestamp::new(0),
            segment_end_ts: ClientTimestamp::new(2_000),
            last_client_ts: ClientTimestamp::new(0),
        });
        let activity = PlayerActivityComp {
            last_movement_start_at: Some(SimInstant::ZERO),
            ..Default::default()
        };

        assert_eq!(
            player_stat_activity(
                SimInstant::from_millis(1_000),
                player_movement_end_at(&motion, &activity),
                &activity,
            )
            .movement,
            PlayerMovementActivity::Moving
        );
    }

    #[test]
    fn finished_movement_age_uses_server_clock_not_client_epoch() {
        let motion = PlayerMotion(crate::runtime::state::PlayerMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(4.0, 1.0),
            segment_start_ts: ClientTimestamp::new(10_000),
            segment_end_ts: ClientTimestamp::new(12_000),
            last_client_ts: ClientTimestamp::new(10_000),
        });
        let activity = PlayerActivityComp {
            last_movement_start_at: Some(SimInstant::from_millis(3_600_000)),
            ..Default::default()
        };

        assert_eq!(
            player_stat_activity(
                SimInstant::from_millis(3_605_000),
                player_movement_end_at(&motion, &activity),
                &activity,
            )
            .movement,
            PlayerMovementActivity::stopped_for(std::time::Duration::from_millis(3_000))
        );
    }

    #[test]
    fn ticker_clock_advances_on_its_own_cadence() {
        let mut clock = SimTickerClock::scheduled(
            SimInstant::ZERO,
            SimInstant::from_millis(10),
            SimDuration::from_millis(7),
        );

        assert_eq!(clock.advance_due(SimInstant::from_millis(9)), None);
        assert_eq!(
            clock.advance_due(SimInstant::from_millis(10)),
            Some(SimDuration::from_millis(10))
        );
        assert_eq!(clock.next_due_at(), SimInstant::from_millis(17));
        assert_eq!(clock.advance_due(SimInstant::from_millis(16)), None);
        assert_eq!(
            clock.advance_due(SimInstant::from_millis(17)),
            Some(SimDuration::from_millis(7))
        );
        assert_eq!(clock.next_due_at(), SimInstant::from_millis(24));
    }
}
