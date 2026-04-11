use super::Stat;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceApplication {
    pub stat: Stat,
    pub delta: i32,
    pub min_value: i32,
    pub clamp_to_cap: bool,
}

impl ResourceApplication {
    pub const fn new(stat: Stat, delta: i32) -> Self {
        Self {
            stat,
            delta,
            min_value: 0,
            clamp_to_cap: true,
        }
    }

    pub const fn with_min_value(mut self, min_value: i32) -> Self {
        self.min_value = min_value;
        self
    }

    pub const fn with_unclamped_cap(mut self) -> Self {
        self.clamp_to_cap = false;
        self
    }

    pub const fn restore(stat: Stat, amount: i32) -> Self {
        Self::new(stat, amount)
    }

    pub const fn spend(stat: Stat, amount: i32) -> Self {
        Self::new(stat, -amount)
    }

    pub const fn damage(amount: i32) -> Self {
        Self::spend(Stat::Hp, amount)
    }

    pub const fn nonlethal_damage(amount: i32, min_remaining_hp: i32) -> Self {
        Self::damage(amount).with_min_value(min_remaining_hp)
    }

    pub const fn poison(amount: i32) -> Self {
        Self::nonlethal_damage(amount, 1)
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceApplicationResult {
    pub stat: Stat,
    pub previous: i32,
    pub current: i32,
    pub applied_delta: i32,
    pub was_clamped: bool,
}

impl ResourceApplicationResult {
    pub fn is_noop(&self) -> bool {
        self.applied_delta == 0
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedRecovery {
    pub stat: Stat,
    pub previous_pending: i32,
    pub current_pending: i32,
    pub queued_amount: i32,
}
