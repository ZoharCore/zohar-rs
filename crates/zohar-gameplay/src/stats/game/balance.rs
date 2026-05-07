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

pub const fn exp_reward_bonus_malus_percent(player_level: i32, monster_level: i32) -> i32 {
    let monster_lvl_advantage = monster_level.saturating_sub(player_level);

    match monster_lvl_advantage {
        // capped linear bonus: player lower level than mob (player handicap)
        15.. => 180,
        0..=14 => 100 + monster_lvl_advantage * 5,

        // capped nonlinear malus: player higher level than mob (player advantage)
        -1 => 100,
        -2 => 98,
        -3 => 96,
        -4 => 94,
        -5 => 92,
        -6 => 90,

        -7 => 85,
        -8 => 80,

        -9 => 70,
        -10 => 50,
        -11 => 30,

        -12 => 20,
        -13 => 10,

        -14 => 5,

        ..=-15 => 1,
    }
}
