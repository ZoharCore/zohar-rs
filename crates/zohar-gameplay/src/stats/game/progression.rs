/// Four client-visible level-step balls make up a full level.
pub const LEVEL_STEPS_PER_LEVEL: i32 = 4;
/// The final level step is represented by a level-up, not an extra stat point.
pub const STAT_POINT_STEPS_PER_LEVEL: i32 = LEVEL_STEPS_PER_LEVEL - 1;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerProgressionState {
    pub level: i32,
    pub exp_in_level: u32,
    pub next_exp_in_level: u32,
}

impl PlayerProgressionState {
    pub const fn new(level: i32, exp_in_level: u32, next_exp_in_level: u32) -> Self {
        Self {
            level,
            exp_in_level,
            next_exp_in_level,
        }
    }

    pub const fn level_only(level: i32) -> Self {
        Self::new(level, 0, 0)
    }

    pub fn normalized(self) -> Self {
        Self {
            level: self.level.max(0),
            exp_in_level: self.exp_in_level,
            next_exp_in_level: self.next_exp_in_level,
        }
    }

    pub fn level_step(self) -> i32 {
        let progression = self.normalized();
        if progression.next_exp_in_level == 0 {
            return 0;
        }

        let exp = u64::from(progression.exp_in_level.min(progression.next_exp_in_level));
        let next_exp = u64::from(progression.next_exp_in_level);
        let level_steps = LEVEL_STEPS_PER_LEVEL as u64;
        ((exp.saturating_mul(level_steps)) / next_exp).min(level_steps) as i32
    }
}
