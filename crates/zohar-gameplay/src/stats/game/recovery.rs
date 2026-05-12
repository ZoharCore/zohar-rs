use std::time::Duration;

use zohar_domain::entity::MovementAnimation;

use super::Stat;
use super::resource::ResourceApplication;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DurationRate {
    amount: i64,
    per: Duration,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RateAccumulator {
    remainder: i128,
}

impl RateAccumulator {
    fn accrue(&mut self, rate: DurationRate, elapsed: Duration) -> i64 {
        let period_nanos = duration_nanos(rate.per);
        if rate.amount <= 0 || period_nanos <= 0 {
            self.remainder = 0;
            return 0;
        }

        let elapsed_nanos = duration_nanos(elapsed);
        if elapsed_nanos <= 0 {
            return 0;
        }

        let accrued = i128::from(rate.amount).saturating_mul(elapsed_nanos);
        let total = self.remainder.saturating_add(accrued);
        let whole = total / period_nanos;
        self.remainder = total % period_nanos;
        whole.min(i128::from(i64::MAX)) as i64
    }

    fn reset(&mut self) {
        self.remainder = 0;
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerPassiveHpRecoveryState {
    accumulator: RateAccumulator,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerPassiveSpRecoveryState {
    accumulator: RateAccumulator,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerStaminaState {
    accumulator: RateAccumulator,
    consuming: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerStatTickerInput {
    resources: PlayerRecoveryResources,
    activity: PlayerStatActivity,
    sp_profile: PlayerSpRecoveryProfile,
    movement_mode: MovementAnimation,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerRecoveryResources {
    hp: i32,
    max_hp: i32,
    sp: i32,
    max_sp: i32,
    stamina: i32,
    max_stamina: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerStatActivity {
    pub movement: PlayerMovementActivity,
    pub since_walk_started: Option<Duration>,
    pub since_attack: Option<Duration>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlayerMovementActivity {
    Moving,
    Stopped(Duration),
    #[default]
    Idle,
}

impl PlayerMovementActivity {
    pub fn stopped_for(since_finished: Duration) -> Self {
        Self::Stopped(since_finished)
    }

    fn moved_recently(self, threshold: Duration) -> bool {
        match self {
            Self::Moving => true,
            Self::Stopped(since_finished) => since_finished < threshold,
            Self::Idle => false,
        }
    }

    fn stopped_for_at_least(self, threshold: Duration) -> bool {
        match self {
            Self::Moving => false,
            Self::Stopped(since_finished) => since_finished >= threshold,
            Self::Idle => true,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSpRecoveryProfile {
    #[default]
    Standard,
    Caster,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PlayerStatTickerOutput {
    hp: Option<ResourceApplication>,
    sp: Option<ResourceApplication>,
    stamina: Option<ResourceApplication>,
    stamina_timer: Option<PlayerStaminaTimerCommand>,
    movement_override: Option<PlayerStaminaMovementOverride>,
}

#[cfg(test)]
impl PlayerStatTickerOutput {
    fn applications(self) -> impl Iterator<Item = ResourceApplication> {
        [self.hp, self.sp, self.stamina].into_iter().flatten()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStaminaTimerCommand {
    Start { consume_per_sec: i32 },
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerStaminaEffect {
    pub application: Option<ResourceApplication>,
    pub timer: Option<PlayerStaminaTimerCommand>,
    pub movement_override: Option<PlayerStaminaMovementOverride>,
}

pub fn tick_player_passive_hp_recovery(
    state: &mut PlayerPassiveHpRecoveryState,
    hp: i32,
    max_hp: i32,
    activity: PlayerStatActivity,
    elapsed: Duration,
) -> Option<ResourceApplication> {
    accrue_passive_hp(state, hp, max_hp, activity, elapsed)
        .map(|amount| ResourceApplication::restore(Stat::Hp, amount))
}

pub fn tick_player_passive_sp_recovery(
    state: &mut PlayerPassiveSpRecoveryState,
    hp: i32,
    max_hp: i32,
    sp: i32,
    max_sp: i32,
    activity: PlayerStatActivity,
    profile: PlayerSpRecoveryProfile,
    elapsed: Duration,
) -> Option<ResourceApplication> {
    accrue_passive_sp(state, hp, max_hp, sp, max_sp, activity, profile, elapsed)
        .map(|amount| ResourceApplication::restore(Stat::Sp, amount))
}

pub fn tick_player_stamina(
    state: &mut PlayerStaminaState,
    stamina: i32,
    max_stamina: i32,
    activity: PlayerStatActivity,
    movement_mode: MovementAnimation,
    elapsed: Duration,
) -> PlayerStaminaEffect {
    let output = tick_stamina(
        state,
        stamina,
        max_stamina,
        activity,
        movement_mode,
        elapsed,
    );

    PlayerStaminaEffect {
        application: output.application,
        timer: output.timer,
        movement_override: output.movement_override,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PlayerStaminaOutput {
    application: Option<ResourceApplication>,
    timer: Option<PlayerStaminaTimerCommand>,
    movement_override: Option<PlayerStaminaMovementOverride>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStaminaMovementOverride {
    ForceWalk,
    RevertToPreferred,
}

fn tick_stamina(
    state: &mut PlayerStaminaState,
    current: i32,
    max: i32,
    activity: PlayerStatActivity,
    movement_mode: MovementAnimation,
    elapsed: Duration,
) -> PlayerStaminaOutput {
    let stamina = LegacyStaminaSnapshot::new(current, max, activity, movement_mode);

    if stamina.should_consume() {
        return consume_running_stamina(state, stamina.current, elapsed);
    }

    let timer = stop_stamina_consumption(state);
    let restore = stamina.restore();

    PlayerStaminaOutput {
        application: restore.map(LegacyStaminaRestore::application),
        timer,
        movement_override: restore.and_then(LegacyStaminaRestore::movement_override),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegacyStaminaSnapshot {
    current: i32,
    max: i32,
    movement: LegacyStaminaMovement,
    inside_combat_window: bool,
}

impl LegacyStaminaSnapshot {
    fn new(
        current: i32,
        max: i32,
        activity: PlayerStatActivity,
        movement_mode: MovementAnimation,
    ) -> Self {
        const COMBAT_WINDOW: Duration = Duration::from_secs(20);

        Self {
            current,
            max,
            movement: LegacyStaminaMovement::new(activity, movement_mode, current),
            inside_combat_window: happened_recently(activity.since_attack, COMBAT_WINDOW),
        }
    }

    fn should_consume(self) -> bool {
        self.inside_combat_window && self.movement.is_combat_run()
    }

    fn restore(self) -> Option<LegacyStaminaRestore> {
        if self.current >= self.max {
            return None;
        }

        self.movement.restore(self.max, self.is_depleted())
    }

    fn is_depleted(self) -> bool {
        self.current <= 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyStaminaMovement {
    Moving { mode: LegacyStaminaMoveMode },
    Stopped { movement: PlayerMovementActivity },
}

impl LegacyStaminaMovement {
    fn new(
        activity: PlayerStatActivity,
        movement_mode: MovementAnimation,
        current_stamina: i32,
    ) -> Self {
        let mode = LegacyStaminaMoveMode::new(activity, movement_mode, current_stamina);
        match activity.movement {
            PlayerMovementActivity::Moving => Self::Moving { mode },
            movement => Self::Stopped { movement },
        }
    }

    fn is_combat_run(self) -> bool {
        matches!(
            self,
            Self::Moving {
                mode: LegacyStaminaMoveMode::Run
            }
        )
    }

    fn restore(self, amount: i32, depleted_before_restore: bool) -> Option<LegacyStaminaRestore> {
        const STOPPED_RESTORE_DELAY: Duration = Duration::from_secs(3);

        match self {
            Self::Moving { mode } => mode.restore(amount, depleted_before_restore),
            Self::Stopped { movement } => movement
                .stopped_for_at_least(STOPPED_RESTORE_DELAY)
                .then_some(LegacyStaminaRestore::Stopped {
                    amount,
                    depleted_before_restore,
                }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyStaminaMoveMode {
    Run,
    Walk { since_started: Option<Duration> },
}

impl LegacyStaminaMoveMode {
    fn new(
        activity: PlayerStatActivity,
        movement_mode: MovementAnimation,
        current_stamina: i32,
    ) -> Self {
        if movement_mode == MovementAnimation::Walk || current_stamina <= 0 {
            Self::Walk {
                since_started: activity.since_walk_started,
            }
        } else {
            Self::Run
        }
    }

    fn restore(self, amount: i32, depleted_before_restore: bool) -> Option<LegacyStaminaRestore> {
        const WALKING_RESTORE_DELAY: Duration = Duration::from_secs(5);

        let Self::Walk { since_started } = self else {
            return None;
        };
        let restore_ready = since_started.is_none_or(|since| since > WALKING_RESTORE_DELAY);
        restore_ready.then_some(LegacyStaminaRestore::Walking {
            amount,
            depleted_before_restore,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyStaminaRestore {
    Walking {
        amount: i32,
        depleted_before_restore: bool,
    },
    Stopped {
        amount: i32,
        depleted_before_restore: bool,
    },
}

impl LegacyStaminaRestore {
    fn application(self) -> ResourceApplication {
        ResourceApplication::restore(Stat::Stamina, self.amount())
    }

    fn movement_override(self) -> Option<PlayerStaminaMovementOverride> {
        self.depleted_before_restore()
            .then_some(PlayerStaminaMovementOverride::RevertToPreferred)
    }

    fn amount(self) -> i32 {
        match self {
            Self::Walking { amount, .. } | Self::Stopped { amount, .. } => amount,
        }
    }

    fn depleted_before_restore(self) -> bool {
        match self {
            Self::Walking {
                depleted_before_restore,
                ..
            }
            | Self::Stopped {
                depleted_before_restore,
                ..
            } => depleted_before_restore,
        }
    }
}

fn consume_running_stamina(
    state: &mut PlayerStaminaState,
    current_stamina: i32,
    elapsed: Duration,
) -> PlayerStaminaOutput {
    const STAMINA_CONSUME_PER_SECOND: i32 = 25;

    let spend = state.accumulator.accrue(
        DurationRate {
            amount: i64::from(STAMINA_CONSUME_PER_SECOND),
            per: Duration::from_secs(1),
        },
        elapsed,
    );
    let spend = spend
        .max(0)
        .min(i64::from(current_stamina))
        .min(i64::from(i32::MAX)) as i32;
    let application = (spend > 0).then_some(ResourceApplication::spend(Stat::Stamina, spend));
    let will_deplete = current_stamina.saturating_sub(spend) <= 0;
    let was_consuming = state.consuming;
    let timer = if will_deplete {
        state.consuming = false;
        state.accumulator.reset();
        was_consuming.then_some(PlayerStaminaTimerCommand::Stop)
    } else {
        start_stamina_consumption(state, STAMINA_CONSUME_PER_SECOND)
    };

    PlayerStaminaOutput {
        application,
        timer,
        movement_override: will_deplete.then_some(PlayerStaminaMovementOverride::ForceWalk),
    }
}

fn start_stamina_consumption(
    state: &mut PlayerStaminaState,
    consume_per_second: i32,
) -> Option<PlayerStaminaTimerCommand> {
    if state.consuming {
        return None;
    }

    state.consuming = true;
    Some(PlayerStaminaTimerCommand::Start {
        consume_per_sec: consume_per_second,
    })
}

fn stop_stamina_consumption(state: &mut PlayerStaminaState) -> Option<PlayerStaminaTimerCommand> {
    state.accumulator.reset();
    if !state.consuming {
        return None;
    }

    state.consuming = false;
    Some(PlayerStaminaTimerCommand::Stop)
}

fn accrue_passive_hp(
    state: &mut PlayerPassiveHpRecoveryState,
    hp: i32,
    max_hp: i32,
    activity: PlayerStatActivity,
    elapsed: Duration,
) -> Option<i32> {
    let missing = max_hp.saturating_sub(hp);
    if missing <= 0 {
        state.accumulator.reset();
        return None;
    }

    let rate = passive_hp_rate(max_hp, activity);
    let accrued = state.accumulator.accrue(
        DurationRate {
            amount: rate.amount,
            per: rate.per,
        },
        elapsed,
    );
    if accrued <= 0 {
        return None;
    }

    let amount = accrued.min(i64::from(missing)).min(i64::from(i32::MAX)) as i32;
    if amount >= missing {
        state.accumulator.reset();
    }
    (amount > 0).then_some(amount)
}

fn accrue_passive_sp(
    state: &mut PlayerPassiveSpRecoveryState,
    hp: i32,
    max_hp: i32,
    sp: i32,
    max_sp: i32,
    activity: PlayerStatActivity,
    profile: PlayerSpRecoveryProfile,
    elapsed: Duration,
) -> Option<i32> {
    let missing = max_sp.saturating_sub(sp);
    if missing <= 0 {
        state.accumulator.reset();
        return None;
    }

    let rate = passive_sp_rate(hp, max_hp, max_sp, activity, profile);
    let accrued = state.accumulator.accrue(
        DurationRate {
            amount: rate.amount,
            per: rate.per,
        },
        elapsed,
    );
    if accrued <= 0 {
        return None;
    }

    let amount = accrued.min(i64::from(missing)).min(i64::from(i32::MAX)) as i32;
    if amount >= missing {
        state.accumulator.reset();
    }
    (amount > 0).then_some(amount)
}

fn passive_hp_rate(max_hp: i32, activity: PlayerStatActivity) -> DurationRate {
    const RECENT_ACTIVITY_WINDOW: Duration = Duration::from_secs(3);

    let moved_recently = moved_recently(activity, RECENT_ACTIVITY_WINDOW);
    let (hp_base, hp_max_share): (i64, _) = if moved_recently {
        (15, FractionalShare::from_percent(1))
    } else {
        (15, FractionalShare::from_percent(5))
    };

    DurationRate {
        amount: hp_base.saturating_add(hp_max_share.of_resource(max_hp)),
        per: Duration::from_secs(3),
    }
}

fn passive_sp_rate(
    hp: i32,
    max_hp: i32,
    max_sp: i32,
    activity: PlayerStatActivity,
    profile: PlayerSpRecoveryProfile,
) -> DurationRate {
    const RECENT_ACTIVITY_WINDOW: Duration = Duration::from_secs(3);

    let moved_recently = moved_recently(activity, RECENT_ACTIVITY_WINDOW);
    let attacked_within_reduced_recovery_window =
        happened_recently(activity.since_attack, RECENT_ACTIVITY_WINDOW);
    let is_full_hp = hp >= max_hp;

    let (sp_base, sp_max_share): (i64, _) = match profile {
        PlayerSpRecoveryProfile::Caster => {
            if attacked_within_reduced_recovery_window {
                (2, FractionalShare::from_basis_points(50))
            } else if moved_recently {
                (3, FractionalShare::from_percent(2))
            } else {
                (10, FractionalShare::from_percent(3))
            }
        }
        PlayerSpRecoveryProfile::Standard => {
            if attacked_within_reduced_recovery_window {
                (2, FractionalShare::from_basis_points(50))
            } else if moved_recently || !is_full_hp {
                (2, FractionalShare::from_percent(1))
            } else {
                (9, FractionalShare::from_percent(1))
            }
        }
    };

    DurationRate {
        amount: sp_base.saturating_add(sp_max_share.of_resource(max_sp)),
        per: Duration::from_secs(3),
    }
}

fn happened_recently(since: Option<Duration>, threshold: Duration) -> bool {
    since.is_some_and(|since| since < threshold)
}

fn moved_recently(activity: PlayerStatActivity, threshold: Duration) -> bool {
    activity.movement.moved_recently(threshold)
}

fn duration_nanos(duration: Duration) -> i128 {
    duration.as_nanos().min(i128::MAX as u128) as i128
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FractionalShare(i64);

impl FractionalShare {
    const BASIS_POINTS_DENOMINATOR: i64 = 10_000;

    const fn from_basis_points(value: i64) -> Self {
        Self(value)
    }

    const fn from_percent(pct: i64) -> Self {
        Self::from_basis_points(pct * 100)
    }

    fn of_resource(self, max_resource: i32) -> i64 {
        i64::from(max_resource).saturating_mul(self.0) / Self::BASIS_POINTS_DENOMINATOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestTickerState {
        passive_hp: PlayerPassiveHpRecoveryState,
        passive_sp: PlayerPassiveSpRecoveryState,
        stamina: PlayerStaminaState,
    }

    fn input(hp: i32, max_hp: i32, sp: i32, max_sp: i32) -> PlayerStatTickerInput {
        PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                hp,
                max_hp,
                sp,
                max_sp,
                stamina: 800,
                max_stamina: 800,
            },
            activity: PlayerStatActivity::default(),
            sp_profile: PlayerSpRecoveryProfile::Standard,
            movement_mode: MovementAnimation::Run,
        }
    }

    fn application_delta(output: PlayerStatTickerOutput, stat: Stat) -> Option<i32> {
        output
            .applications()
            .find(|application| application.stat == stat)
            .map(|application| application.delta)
    }

    fn tick_player_stat_tickers(
        state: &mut TestTickerState,
        input: PlayerStatTickerInput,
        elapsed: Duration,
    ) -> PlayerStatTickerOutput {
        let hp = tick_player_passive_hp_recovery(
            &mut state.passive_hp,
            input.resources.hp,
            input.resources.max_hp,
            input.activity,
            elapsed,
        );
        let sp = tick_player_passive_sp_recovery(
            &mut state.passive_sp,
            input.resources.hp,
            input.resources.max_hp,
            input.resources.sp,
            input.resources.max_sp,
            input.activity,
            input.sp_profile,
            elapsed,
        );
        let stamina = tick_player_stamina(
            &mut state.stamina,
            input.resources.stamina,
            input.resources.max_stamina,
            input.activity,
            input.movement_mode,
            elapsed,
        );

        PlayerStatTickerOutput {
            hp,
            sp,
            stamina: stamina.application,
            stamina_timer: stamina.timer,
            movement_override: stamina.movement_override,
        }
    }

    #[test]
    fn passive_hp_restores_legacy_resting_amount_over_three_seconds() {
        let mut state = TestTickerState::default();
        let input = input(500, 1_000, 200, 200);

        let first = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));
        let second = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));
        let third = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        let restored: i32 = first
            .applications()
            .chain(second.applications())
            .chain(third.applications())
            .map(|application| application.delta)
            .sum();
        assert_eq!(restored, 65);
    }

    #[test]
    fn passive_hp_uses_reduced_percent_while_moving_recently() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::ZERO),
                ..Default::default()
            },
            ..input(500, 1_000, 200, 200)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Hp), Some(25));
    }

    #[test]
    fn passive_hp_uses_reduced_percent_while_currently_moving() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                ..Default::default()
            },
            ..input(500, 1_000, 200, 200)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Hp), Some(25));
    }

    #[test]
    fn passive_hp_resets_fractional_carry_when_recovery_deactivates() {
        let mut state = TestTickerState::default();
        let active = input(500, 1_000, 200, 200);
        let inactive = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                hp: 1_000,
                ..active.resources
            },
            ..active
        };

        let first = tick_player_stat_tickers(&mut state, active, Duration::from_secs(1));
        let _ = tick_player_stat_tickers(&mut state, inactive, Duration::from_secs(1));
        let output = tick_player_stat_tickers(&mut state, active, Duration::from_secs(1));

        assert_eq!(output, first);
    }

    #[test]
    fn passive_hp_recovers_zero_hp_when_runtime_life_allows_ticking() {
        let mut state = TestTickerState::default();
        let input = input(0, 1_000, 200, 200);

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Hp), Some(65));
    }

    #[test]
    fn passive_sp_restores_standard_legacy_resting_amount_when_hp_is_full() {
        let mut state = TestTickerState::default();
        let input = input(1_000, 1_000, 100, 300);

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Sp), Some(12));
    }

    #[test]
    fn passive_sp_uses_lower_standard_resting_amount_while_hp_is_missing() {
        let mut state = TestTickerState::default();
        let input = input(900, 1_000, 100, 300);

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Sp), Some(5));
    }

    #[test]
    fn passive_sp_recovers_zero_hp_when_runtime_life_allows_ticking() {
        let mut state = TestTickerState::default();
        let input = input(0, 1_000, 100, 300);

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Sp), Some(5));
    }

    #[test]
    fn passive_sp_uses_moving_amount_while_currently_moving() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                ..Default::default()
            },
            ..input(1_000, 1_000, 100, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Sp), Some(5));
    }

    #[test]
    fn passive_sp_uses_caster_legacy_resting_amount() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            sp_profile: PlayerSpRecoveryProfile::Caster,
            ..input(900, 1_000, 100, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(3));

        assert_eq!(application_delta(output, Stat::Sp), Some(19));
    }

    #[test]
    fn stamina_consumes_while_running_in_combat_and_starts_client_timer() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::ZERO),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(-25));
        assert_eq!(
            output.stamina_timer,
            Some(PlayerStaminaTimerCommand::Start {
                consume_per_sec: 25
            })
        );
    }

    #[test]
    fn stamina_continues_consuming_without_restarting_client_timer() {
        let mut state = TestTickerState {
            stamina: PlayerStaminaState {
                consuming: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::ZERO),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(-25));
        assert_eq!(output.stamina_timer, None);
    }

    #[test]
    fn stamina_stops_consuming_when_preferred_walk_mode_is_toggled() {
        let mut state = TestTickerState {
            stamina: PlayerStaminaState {
                consuming: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::ZERO),
                since_walk_started: Some(Duration::ZERO),
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
        assert_eq!(output.stamina_timer, Some(PlayerStaminaTimerCommand::Stop));
    }

    #[test]
    fn stamina_stops_consuming_when_movement_ends() {
        let mut state = TestTickerState {
            stamina: PlayerStaminaState {
                consuming: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::ZERO),
                since_attack: Some(Duration::ZERO),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
        assert_eq!(output.stamina_timer, Some(PlayerStaminaTimerCommand::Stop));
    }

    #[test]
    fn stamina_stops_consuming_at_exact_legacy_combat_window() {
        let mut state = TestTickerState {
            stamina: PlayerStaminaState {
                consuming: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let input = PlayerStatTickerInput {
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::from_secs(20)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
        assert_eq!(output.stamina_timer, Some(PlayerStaminaTimerCommand::Stop));
    }

    #[test]
    fn stamina_restores_while_walking_after_legacy_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_walk_started: Some(Duration::from_millis(5_001)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(800));
        // Partial stamina (not zero) should not trigger walk mode override
        assert_eq!(output.movement_override, None);
    }

    #[test]
    fn stamina_recovers_zero_hp_when_runtime_life_allows_ticking() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                hp: 0,
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::from_secs(3)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(800));
    }

    #[test]
    fn stamina_does_not_restore_at_exact_legacy_walk_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_walk_started: Some(Duration::from_secs(5)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
    }

    #[test]
    fn stamina_depletion_forces_walk_mode_override() {
        let mut state = TestTickerState::default();

        // 25 stamina left, consuming 25/sec → will deplete in one tick
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 25,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::ZERO),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(-25));
        assert_eq!(
            output.movement_override,
            Some(PlayerStaminaMovementOverride::ForceWalk)
        );
        // Timer was never started, so no Stop command
        assert_eq!(output.stamina_timer, None);
    }

    #[test]
    fn stamina_depletion_during_active_timer_stops_timer_and_forces_walk() {
        let mut state = TestTickerState {
            stamina: PlayerStaminaState {
                consuming: true,
                ..Default::default()
            },
            ..Default::default()
        };

        // 25 stamina left, consuming 25/sec → will deplete
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 25,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_attack: Some(Duration::ZERO),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(-25));
        assert_eq!(
            output.movement_override,
            Some(PlayerStaminaMovementOverride::ForceWalk)
        );
        // Timer was running, so Stop is emitted
        assert_eq!(output.stamina_timer, Some(PlayerStaminaTimerCommand::Stop));
    }

    #[test]
    fn stamina_restoration_from_zero_reverts_to_preferred_mode() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 0,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_walk_started: Some(Duration::from_millis(5_001)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        // Stamina restored to max
        assert_eq!(application_delta(output, Stat::Stamina), Some(800));
        // Should signal revert to preferred (run) mode
        assert_eq!(
            output.movement_override,
            Some(PlayerStaminaMovementOverride::RevertToPreferred)
        );
    }

    #[test]
    fn stamina_restores_while_idle_after_legacy_stop_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::from_millis(5_001)),
                since_walk_started: Some(Duration::from_millis(5_001)),
                since_attack: Some(Duration::from_secs(30)),
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(800));
        assert_eq!(output.movement_override, None);
    }

    #[test]
    fn stamina_does_not_restore_before_legacy_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_walk_started: Some(Duration::from_secs(3)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        // 3s < 5s delay, no restoration yet
        assert_eq!(application_delta(output, Stat::Stamina), None);
    }

    #[test]
    fn stamina_restores_after_legacy_stop_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::from_secs(3)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), Some(800));
        assert_eq!(output.movement_override, None);
    }

    #[test]
    fn stamina_does_not_restore_before_legacy_stop_delay() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::stopped_for(Duration::from_millis(2_999)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Run,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
    }

    #[test]
    fn stamina_does_not_restore_soon_after_entering_walk_mode() {
        let mut state = TestTickerState::default();
        let input = PlayerStatTickerInput {
            resources: PlayerRecoveryResources {
                stamina: 300,
                max_stamina: 800,
                ..input(1_000, 1_000, 300, 300).resources
            },
            activity: PlayerStatActivity {
                movement: PlayerMovementActivity::Moving,
                since_walk_started: Some(Duration::from_millis(500)),
                ..Default::default()
            },
            movement_mode: MovementAnimation::Walk,
            ..input(1_000, 1_000, 300, 300)
        };

        let output = tick_player_stat_tickers(&mut state, input, Duration::from_secs(1));

        assert_eq!(application_delta(output, Stat::Stamina), None);
    }
}
