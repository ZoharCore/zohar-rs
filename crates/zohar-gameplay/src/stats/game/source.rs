use super::Stat;
use super::actor::ActorKind;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoreStatBlock {
    pub st: i32,
    pub ht: i32,
    pub dx: i32,
    pub iq: i32,
}

impl CoreStatBlock {
    pub const fn new(st: i32, ht: i32, dx: i32, iq: i32) -> Self {
        Self { st, ht, dx, iq }
    }

    pub const fn get(self, stat: Stat) -> i32 {
        match stat {
            Stat::St => self.st,
            Stat::Ht => self.ht,
            Stat::Dx => self.dx,
            Stat::Iq => self.iq,
            _ => 0,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpeeds {
    pub move_speed: i32,
    pub attack_speed: i32,
    pub casting_speed: i32,
}

impl Default for SourceSpeeds {
    fn default() -> Self {
        Self {
            move_speed: 100,
            attack_speed: 100,
            casting_speed: 100,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAttackCoefficients {
    pub st_numerator: i32,
    pub dx_numerator: i32,
    pub iq_numerator: i32,
    pub divisor: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerResourceFormula {
    pub base_max_hp: i32,
    pub base_max_sp: i32,
    pub base_max_stamina: i32,
    pub hp_per_ht: i32,
    pub sp_per_iq: i32,
    pub stamina_per_ht: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlayerResourceCapacity {
    Hp,
    Sp,
    Stamina,
}

impl PlayerResourceCapacity {
    pub const fn cap_stat(self) -> Stat {
        match self {
            Self::Hp => Stat::MaxHp,
            Self::Sp => Stat::MaxSp,
            Self::Stamina => Stat::MaxStamina,
        }
    }

    pub const fn flat_bonus_stat(self) -> Option<Stat> {
        match self {
            Self::Hp => Some(Stat::BonusMaxHp),
            Self::Sp => Some(Stat::BonusMaxSp),
            Self::Stamina => Some(Stat::BonusMaxStamina),
        }
    }

    pub const fn pre_percentage_bonus_stat(self) -> Option<Stat> {
        match self {
            Self::Hp => Some(Stat::MaxHpPrePctBonus),
            Self::Sp | Self::Stamina => None,
        }
    }

    pub const fn capped_percentage_stat(self) -> Option<Stat> {
        match self {
            Self::Hp => Some(Stat::MaxHpPct),
            Self::Sp => Some(Stat::MaxSpPct),
            Self::Stamina => None,
        }
    }

    pub const fn post_percentage_bonus_stat(self) -> Option<Stat> {
        match self {
            Self::Hp => Some(Stat::PartyTankerBonus),
            Self::Sp => Some(Stat::PartySkillMasterBonus),
            Self::Stamina => None,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorViewLimits {
    pub move_speed_max: i32,
    pub attack_speed_max: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerStoredStatLimits {
    pub core_stat_max: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerResourceBonusCaps {
    pub max_hp_pct_bonus: i32,
    pub max_sp_pct_bonus: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerBalanceRules {
    pub attack: PlayerAttackCoefficients,
    pub stored_limits: PlayerStoredStatLimits,
    pub view_limits: ActorViewLimits,
    pub resource_bonus_caps: PlayerResourceBonusCaps,
}

impl PlayerBalanceRules {
    pub const fn view_limit(self, stat: Stat) -> Option<i32> {
        match stat {
            Stat::MovSpeed => Some(self.view_limits.move_speed_max),
            Stat::AttSpeed => Some(self.view_limits.attack_speed_max),
            _ => None,
        }
    }

    pub const fn stored_write_limit(self, stat: Stat) -> Option<i32> {
        match stat {
            Stat::St | Stat::Ht | Stat::Dx | Stat::Iq => Some(self.stored_limits.core_stat_max),
            _ => None,
        }
    }

    pub(crate) const fn capped_percentage_bonus_max(
        self,
        resource: PlayerResourceCapacity,
    ) -> Option<i32> {
        match resource {
            PlayerResourceCapacity::Hp => Some(self.resource_bonus_caps.max_hp_pct_bonus),
            PlayerResourceCapacity::Sp => Some(self.resource_bonus_caps.max_sp_pct_bonus),
            PlayerResourceCapacity::Stamina => None,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobBalanceRules {
    pub view_limits: ActorViewLimits,
}

impl MobBalanceRules {
    pub const fn view_limit(self, stat: Stat) -> Option<i32> {
        match stat {
            Stat::MovSpeed => Some(self.view_limits.move_speed_max),
            Stat::AttSpeed => Some(self.view_limits.attack_speed_max),
            _ => None,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeterministicGrowthVersion {
    V1,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerGrowthFormula {
    pub hp_per_level: (i32, i32),
    pub sp_per_level: (i32, i32),
    pub stamina_per_level: (i32, i32),
    pub version: DeterministicGrowthVersion,
}

impl PlayerGrowthFormula {
    pub const fn zero() -> Self {
        Self {
            hp_per_level: (0, 0),
            sp_per_level: (0, 0),
            stamina_per_level: (0, 0),
            version: DeterministicGrowthVersion::V1,
        }
    }

    pub fn random_hp(self, stable_id: u64, level: i32) -> i32 {
        self.sum_growth_rolls(b"grow::HP", stable_id, level, self.hp_per_level)
    }

    pub fn random_sp(self, stable_id: u64, level: i32) -> i32 {
        self.sum_growth_rolls(b"grow::SP", stable_id, level, self.sp_per_level)
    }

    pub fn random_stamina(self, stable_id: u64, level: i32) -> i32 {
        self.sum_growth_rolls(b"grow:STM", stable_id, level, self.stamina_per_level)
    }

    fn sum_growth_rolls(
        self,
        domain_salt: &[u8; 8],
        stable_id: u64,
        level: i32,
        (min, max): (i32, i32),
    ) -> i32 {
        let rolls = level.saturating_sub(1) as usize;
        if rolls == 0 {
            return 0;
        }

        let (lower, upper) = if min <= max { (min, max) } else { (max, min) };

        match self.version {
            DeterministicGrowthVersion::V1 => {
                let mut rng = <rand_xoshiro::SplitMix64 as rand::SeedableRng>::seed_from_u64(
                    u64::from_be_bytes(*b"GROWTHv1") ^ u64::from_be_bytes(*domain_salt) ^ stable_id,
                );
                let growth_distribution =
                    rand::distr::Uniform::new_inclusive(lower, upper).unwrap();

                rand::distr::Distribution::sample_iter(growth_distribution, &mut rng)
                    .take(rolls)
                    .sum()
            }
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerStatSource {
    pub resources: PlayerResourceFormula,
    pub growth: PlayerGrowthFormula,
    pub balance: PlayerBalanceRules,
    pub speeds: SourceSpeeds,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobStatSource {
    pub level: i32,
    pub core: CoreStatBlock,
    pub max_hp: i32,
    pub def_grade_flat: i32,
    pub balance: MobBalanceRules,
    pub speeds: SourceSpeeds,
}

impl MobStatSource {
    pub const fn base_core_stat(self, stat: Stat) -> i32 {
        match stat {
            Stat::Level => self.level,
            _ => self.core.get(stat),
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorStatSource {
    Player(PlayerStatSource),
    Mob(MobStatSource),
}

impl ActorStatSource {
    pub const fn actor_kind(self) -> ActorKind {
        match self {
            Self::Player(_) => ActorKind::Player,
            Self::Mob(_) => ActorKind::Mob,
        }
    }

    pub const fn player(self) -> Option<PlayerStatSource> {
        match self {
            Self::Player(source) => Some(source),
            Self::Mob(_) => None,
        }
    }

    pub const fn mob(self) -> Option<MobStatSource> {
        match self {
            Self::Player(_) => None,
            Self::Mob(source) => Some(source),
        }
    }

    pub const fn base_core_stat(self, stat: Stat) -> i32 {
        match self {
            Self::Player(_) => 0,
            Self::Mob(source) => source.base_core_stat(stat),
        }
    }

    pub const fn view_limit(self, stat: Stat) -> Option<i32> {
        match self {
            Self::Player(source) => source.balance.view_limit(stat),
            Self::Mob(source) => source.balance.view_limit(stat),
        }
    }

    pub const fn stored_write_limit(self, stat: Stat) -> Option<i32> {
        match self {
            Self::Player(source) => source.balance.stored_write_limit(stat),
            Self::Mob(_) => None,
        }
    }
}
