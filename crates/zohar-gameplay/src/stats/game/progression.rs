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

    pub fn quarter_chunks_level_step(self) -> i32 {
        let progression = self.normalized();
        if progression.next_exp_in_level == 0 {
            return 0;
        }

        let quarter = progression.next_exp_in_level / 4;
        if progression.exp_in_level >= progression.next_exp_in_level {
            4
        } else if progression.exp_in_level >= quarter * 3 {
            3
        } else if progression.exp_in_level >= quarter * 2 {
            2
        } else if progression.exp_in_level >= quarter {
            1
        } else {
            0
        }
    }
}
