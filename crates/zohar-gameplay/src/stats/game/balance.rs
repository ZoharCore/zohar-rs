use zohar_domain::entity::player::PlayerClass;

use super::source::{
    ActorViewLimits, MobBalanceRules, PlayerAttackCoefficients, PlayerBalanceRules,
    PlayerResourceBonusCaps, PlayerStoredStatLimits,
};

pub const fn default_player_balance_rules(class: PlayerClass) -> PlayerBalanceRules {
    PlayerBalanceRules {
        attack: match class {
            PlayerClass::Warrior | PlayerClass::Sura => PlayerAttackCoefficients {
                st_numerator: 2,
                dx_numerator: 0,
                iq_numerator: 0,
                divisor: 1,
            },
            PlayerClass::Ninja => PlayerAttackCoefficients {
                st_numerator: 4,
                dx_numerator: 2,
                iq_numerator: 0,
                divisor: 3,
            },
            PlayerClass::Shaman => PlayerAttackCoefficients {
                st_numerator: 4,
                dx_numerator: 0,
                iq_numerator: 2,
                divisor: 3,
            },
        },
        stored_limits: PlayerStoredStatLimits { core_stat_max: 90 },
        view_limits: ActorViewLimits {
            move_speed_max: 200,
            attack_speed_max: 170,
        },
        resource_bonus_caps: PlayerResourceBonusCaps {
            max_hp_pct_bonus: 3_500,
            max_sp_pct_bonus: 800,
        },
    }
}

pub const fn default_mob_balance_rules() -> MobBalanceRules {
    MobBalanceRules {
        view_limits: ActorViewLimits {
            move_speed_max: 250,
            attack_speed_max: 250,
        },
    }
}
