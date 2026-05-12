use std::collections::HashMap;

use zohar_domain::entity::mob::{MobBattleType, MobId};

const DEFAULT_PROJECTILE_INIT_VEL_UNITS_PER_SEC: u32 = 200;
const DEFAULT_PROJECTILE_BOMB_RANGE_UNITS: u32 = 10;
const FALLBACK_PROJECTILE_INIT_VEL_UNITS_PER_SEC: u32 = 3_000;
const FALLBACK_ATTACK_WINDUP_MIN_MS: u32 = 200;
const FALLBACK_ATTACK_WINDUP_MAX_MS: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobAttackTiming {
    pub proc: MobAttackProcTiming,
    pub action_duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobAttackProcTiming {
    Melee {
        damage_delay_ms: u32,
    },
    Projectile {
        release_delay_ms: u32,
        flight: FlyTiming,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlyTiming {
    pub init_vel_units_per_sec: u32,
    pub forward_accel_units_per_sec2: u32,
    pub bomb_range_units: u32,
}

impl FlyTiming {
    pub const DEFAULT_PROJECTILE: Self = Self {
        init_vel_units_per_sec: DEFAULT_PROJECTILE_INIT_VEL_UNITS_PER_SEC,
        forward_accel_units_per_sec2: 0,
        bomb_range_units: DEFAULT_PROJECTILE_BOMB_RANGE_UNITS,
    };

    pub const FALLBACK_PROJECTILE: Self = Self {
        init_vel_units_per_sec: FALLBACK_PROJECTILE_INIT_VEL_UNITS_PER_SEC,
        forward_accel_units_per_sec2: 0,
        bomb_range_units: DEFAULT_PROJECTILE_BOMB_RANGE_UNITS,
    };
}

#[derive(Debug, Clone, Default)]
pub struct MobAttackTimingTable {
    by_mob: HashMap<MobId, MobAttackTiming>,
}

impl MobAttackTimingTable {
    pub fn timing_for(&self, mob_id: MobId) -> Option<MobAttackTiming> {
        self.by_mob.get(&mob_id).copied()
    }

    pub fn upsert_timing(&mut self, mob_id: MobId, timing: MobAttackTiming) {
        self.by_mob.insert(mob_id, timing);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackDamageTiming {
    Immediate,
    DelayedMs(u32),
    Projectile {
        release_delay_ms: u32,
        flight: FlyTiming,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobAttackEffectTiming {
    pub action_duration_ms: u32,
    pub damage: AttackDamageTiming,
    pub set_projectile_target: bool,
}

pub fn mob_attack_effect_timing(
    battle_type: MobBattleType,
    authored_timing: Option<MobAttackTiming>,
    fallback_action_duration_ms: u32,
) -> MobAttackEffectTiming {
    if let Some(timing) = authored_timing {
        return match timing.proc {
            MobAttackProcTiming::Melee { damage_delay_ms } => MobAttackEffectTiming {
                action_duration_ms: timing.action_duration_ms,
                damage: if damage_delay_ms == 0 {
                    AttackDamageTiming::Immediate
                } else {
                    AttackDamageTiming::DelayedMs(damage_delay_ms)
                },
                set_projectile_target: uses_projectile_attack(battle_type),
            },
            MobAttackProcTiming::Projectile {
                release_delay_ms,
                flight,
            } => MobAttackEffectTiming {
                action_duration_ms: timing.action_duration_ms,
                damage: AttackDamageTiming::Projectile {
                    release_delay_ms,
                    flight,
                },
                set_projectile_target: true,
            },
        };
    }

    let windup_ms = (fallback_action_duration_ms / 2)
        .clamp(FALLBACK_ATTACK_WINDUP_MIN_MS, FALLBACK_ATTACK_WINDUP_MAX_MS);
    if uses_projectile_attack(battle_type) {
        MobAttackEffectTiming {
            action_duration_ms: fallback_action_duration_ms,
            damage: AttackDamageTiming::Projectile {
                release_delay_ms: windup_ms,
                flight: FlyTiming::FALLBACK_PROJECTILE,
            },
            set_projectile_target: true,
        }
    } else {
        MobAttackEffectTiming {
            action_duration_ms: fallback_action_duration_ms,
            damage: AttackDamageTiming::DelayedMs(windup_ms),
            set_projectile_target: false,
        }
    }
}

pub fn projectile_travel_ms(distance_m: f32, flight: FlyTiming) -> u32 {
    let effective_units = (distance_m.max(0.0) * 100.0 - flight.bomb_range_units as f32).max(0.0);
    projectile_travel_time_ms(effective_units, flight)
}

fn projectile_travel_time_ms(distance_units: f32, flight: FlyTiming) -> u32 {
    if distance_units <= 0.0 {
        return 0;
    }

    let velocity = flight.init_vel_units_per_sec.max(1) as f32;
    let acceleration = flight.forward_accel_units_per_sec2 as f32;
    let seconds = if acceleration > 0.0 {
        ((velocity * velocity + 2.0 * acceleration * distance_units).sqrt() - velocity)
            / acceleration
    } else {
        distance_units / velocity
    };

    (seconds.max(0.0) * 1000.0).ceil() as u32
}

#[derive(Debug, Clone)]
pub struct MobAttackMotion {
    pub motion_id: i64,
    pub mob_id: MobId,
    pub weight: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone)]
pub struct MotionHitWindow {
    pub motion_id: i64,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MotionFlyEvent {
    pub motion_id: i64,
    pub release_ms: i64,
    pub fly_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MotionFlyData {
    pub fly_file: String,
    pub init_vel: f64,
    pub bomb_range: f64,
    pub accel_y: f64,
}

pub fn mob_attack_timings_from_motion(
    motions: impl IntoIterator<Item = MobAttackMotion>,
    hit_windows: impl IntoIterator<Item = MotionHitWindow>,
    fly_events: impl IntoIterator<Item = MotionFlyEvent>,
    fly_data: impl IntoIterator<Item = MotionFlyData>,
) -> MobAttackTimingTable {
    #[derive(Debug, Clone, Copy)]
    struct MotionHit {
        start_ms: u32,
        end_ms: u32,
    }

    #[derive(Default)]
    struct MeleeAccumulator {
        latest_start_ms: u32,
        earliest_end_ms: Option<u32>,
        weighted_start_sum: u128,
        total_weight: u128,
        max_duration_ms: u32,
    }

    impl MeleeAccumulator {
        fn push(&mut self, hit: MotionHit, weight: i64, duration_ms: i64) {
            let weight = weight.max(1) as u128;
            self.latest_start_ms = self.latest_start_ms.max(hit.start_ms);
            self.earliest_end_ms = Some(
                self.earliest_end_ms
                    .map_or(hit.end_ms, |existing| existing.min(hit.end_ms)),
            );
            self.weighted_start_sum += u128::from(hit.start_ms) * weight;
            self.total_weight += weight;
            self.max_duration_ms = self
                .max_duration_ms
                .max(duration_ms.clamp(0, i64::from(u32::MAX)) as u32);
        }

        fn timing_parts(self) -> Option<(u32, u32)> {
            if self.total_weight == 0 {
                return None;
            }
            let damage_delay_ms = if self
                .earliest_end_ms
                .is_some_and(|earliest_end_ms| self.latest_start_ms <= earliest_end_ms)
            {
                self.latest_start_ms
            } else {
                (self.weighted_start_sum / self.total_weight).min(u128::from(u32::MAX)) as u32
            };

            Some((damage_delay_ms, self.max_duration_ms))
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct MotionFly {
        release_ms: u32,
        flight: FlyTiming,
    }

    #[derive(Default)]
    struct ProjectileAccumulator {
        earliest_release_ms: Option<u32>,
        max_duration_ms: u32,
        flight: Option<FlyTiming>,
    }

    impl ProjectileAccumulator {
        fn push(&mut self, fly: MotionFly, duration_ms: i64) {
            self.earliest_release_ms = Some(
                self.earliest_release_ms
                    .map_or(fly.release_ms, |existing| existing.min(fly.release_ms)),
            );
            self.max_duration_ms = self
                .max_duration_ms
                .max(duration_ms.clamp(0, i64::from(u32::MAX)) as u32);
            self.flight.get_or_insert(fly.flight);
        }

        fn timing(self) -> Option<(u32, u32, FlyTiming)> {
            Some((
                self.earliest_release_ms?,
                self.max_duration_ms,
                self.flight.unwrap_or(FlyTiming::DEFAULT_PROJECTILE),
            ))
        }
    }

    let first_hit_by_motion = hit_windows
        .into_iter()
        .filter(|window| window.start_ms >= 0)
        .fold(HashMap::<i64, MotionHit>::new(), |mut acc, window| {
            let start_ms = window.start_ms.min(i64::from(u32::MAX)) as u32;
            let end_ms = window
                .end_ms
                .unwrap_or(window.start_ms)
                .max(window.start_ms)
                .min(i64::from(u32::MAX)) as u32;
            let hit = MotionHit { start_ms, end_ms };
            acc.entry(window.motion_id)
                .and_modify(|existing| {
                    if hit.start_ms < existing.start_ms {
                        *existing = hit;
                    }
                })
                .or_insert(hit);
            acc
        });

    let fly_data_by_file = fly_data
        .into_iter()
        .map(|data| {
            (
                data.fly_file,
                FlyTiming {
                    init_vel_units_per_sec: data.init_vel.max(1.0).ceil().min(u32::MAX as f64)
                        as u32,
                    forward_accel_units_per_sec2: (-data.accel_y)
                        .max(0.0)
                        .ceil()
                        .min(u32::MAX as f64)
                        as u32,
                    bomb_range_units: data.bomb_range.max(0.0).ceil().min(u32::MAX as f64) as u32,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let first_fly_by_motion = fly_events
        .into_iter()
        .filter(|event| event.release_ms >= 0)
        .fold(HashMap::<i64, MotionFly>::new(), |mut acc, event| {
            let release_ms = event.release_ms.min(i64::from(u32::MAX)) as u32;
            let flight = event
                .fly_file
                .as_ref()
                .and_then(|fly_file| fly_data_by_file.get(fly_file))
                .copied()
                .unwrap_or(FlyTiming::DEFAULT_PROJECTILE);
            let fly = MotionFly { release_ms, flight };
            acc.entry(event.motion_id)
                .and_modify(|existing| {
                    if fly.release_ms < existing.release_ms {
                        *existing = fly;
                    }
                })
                .or_insert(fly);
            acc
        });

    let mut melee_by_mob = HashMap::<MobId, MeleeAccumulator>::new();
    let mut projectile_by_mob = HashMap::<MobId, ProjectileAccumulator>::new();
    for motion in motions {
        if motion.weight <= 0 {
            continue;
        }

        if let Some(&hit) = first_hit_by_motion.get(&motion.motion_id) {
            melee_by_mob.entry(motion.mob_id).or_default().push(
                hit,
                motion.weight,
                motion.duration_ms,
            );
            continue;
        }
        if let Some(fly) = first_fly_by_motion.get(&motion.motion_id).copied() {
            projectile_by_mob
                .entry(motion.mob_id)
                .or_default()
                .push(fly, motion.duration_ms);
        }
    }

    let mut table = MobAttackTimingTable::default();
    for (mob_id, accumulator) in melee_by_mob {
        if let Some((damage_delay_ms, action_duration_ms)) = accumulator.timing_parts() {
            table.upsert_timing(
                mob_id,
                MobAttackTiming {
                    proc: MobAttackProcTiming::Melee { damage_delay_ms },
                    action_duration_ms,
                },
            );
        }
    }
    for (mob_id, accumulator) in projectile_by_mob {
        if table.timing_for(mob_id).is_some() {
            continue;
        }
        if let Some((release_delay_ms, action_duration_ms, flight)) = accumulator.timing() {
            table.upsert_timing(
                mob_id,
                MobAttackTiming {
                    proc: MobAttackProcTiming::Projectile {
                        release_delay_ms,
                        flight,
                    },
                    action_duration_ms,
                },
            );
        }
    }

    table
}

fn uses_projectile_attack(battle_type: MobBattleType) -> bool {
    matches!(battle_type, MobBattleType::Range | MobBattleType::Magic)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attack_motion(
        motion_id: i64,
        mob_id: u32,
        weight: i64,
        duration_ms: i64,
    ) -> MobAttackMotion {
        MobAttackMotion {
            motion_id,
            mob_id: MobId::new(mob_id),
            weight,
            duration_ms,
        }
    }

    fn hit_window(motion_id: i64, start_ms: i64, end_ms: Option<i64>) -> MotionHitWindow {
        MotionHitWindow {
            motion_id,
            start_ms,
            end_ms,
        }
    }

    fn fly_event(motion_id: i64, release_ms: i64, fly_file: &str) -> MotionFlyEvent {
        MotionFlyEvent {
            motion_id,
            release_ms,
            fly_file: Some(fly_file.to_string()),
        }
    }

    fn fly_data(fly_file: &str, init_vel: f64, bomb_range: f64) -> MotionFlyData {
        MotionFlyData {
            fly_file: fly_file.to_string(),
            init_vel,
            bomb_range,
            accel_y: 0.0,
        }
    }

    #[test]
    fn mob_attack_timing_uses_common_hit_overlap_and_max_duration() {
        let timings = mob_attack_timings_from_motion(
            vec![
                attack_motion(1, 101, 50, 700),
                attack_motion(2, 101, 50, 900),
            ],
            vec![hit_window(1, 369, Some(569)), hit_window(2, 564, Some(711))],
            Vec::new(),
            Vec::new(),
        );

        let timing = timings.timing_for(MobId::new(101)).expect("timing");
        assert_eq!(
            timing.proc,
            MobAttackProcTiming::Melee {
                damage_delay_ms: 564
            }
        );
        assert_eq!(timing.action_duration_ms, 900);
    }

    #[test]
    fn mob_attack_timing_falls_back_to_weighted_average_without_overlap() {
        let timings = mob_attack_timings_from_motion(
            vec![
                attack_motion(1, 101, 25, 700),
                attack_motion(2, 101, 75, 800),
            ],
            vec![hit_window(1, 200, Some(300)), hit_window(2, 600, Some(700))],
            Vec::new(),
            Vec::new(),
        );

        let timing = timings.timing_for(MobId::new(101)).expect("timing");
        assert_eq!(
            timing.proc,
            MobAttackProcTiming::Melee {
                damage_delay_ms: 500
            }
        );
        assert_eq!(timing.action_duration_ms, 800);
    }

    #[test]
    fn mob_attack_timing_uses_earliest_fly_release_when_no_hit_exists() {
        let timings = mob_attack_timings_from_motion(
            vec![
                attack_motion(1, 102, 50, 700),
                attack_motion(2, 102, 50, 900),
            ],
            Vec::new(),
            vec![
                fly_event(1, 600, "c:/arrow.fly"),
                fly_event(2, 320, "c:/arrow.fly"),
            ],
            vec![fly_data("c:/arrow.fly", 650.0, 25.0)],
        );

        let timing = timings.timing_for(MobId::new(102)).expect("timing");
        assert_eq!(
            timing.proc,
            MobAttackProcTiming::Projectile {
                release_delay_ms: 320,
                flight: FlyTiming {
                    init_vel_units_per_sec: 650,
                    forward_accel_units_per_sec2: 0,
                    bomb_range_units: 25,
                },
            }
        );
        assert_eq!(timing.action_duration_ms, 900);
    }

    #[test]
    fn mob_attack_effect_timing_chooses_fallback_projectile_for_ranged_mobs() {
        for battle_type in [MobBattleType::Range, MobBattleType::Magic] {
            let timing = mob_attack_effect_timing(battle_type, None, 900);

            assert_eq!(
                timing,
                MobAttackEffectTiming {
                    action_duration_ms: 900,
                    damage: AttackDamageTiming::Projectile {
                        release_delay_ms: 450,
                        flight: FlyTiming::FALLBACK_PROJECTILE,
                    },
                    set_projectile_target: true,
                }
            );
        }
    }
}
