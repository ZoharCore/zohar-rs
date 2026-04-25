#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatStats {
    pub level: i32,
    pub dx: i32,
    pub attack_grade: i32,
    pub defence_grade: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRolls {
    pub weapon_or_mob_damage: i32,
    pub low_damage_fallback: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitFlags(u8);

impl HitFlags {
    pub const NORMAL: Self = Self(1 << 0);
    pub const POISON: Self = Self(1 << 1);
    pub const DODGE: Self = Self(1 << 2);
    pub const BLOCK: Self = Self(1 << 3);
    pub const PENETRATE: Self = Self(1 << 4);
    pub const CRITICAL: Self = Self(1 << 5);

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalHitInput {
    pub attacker: CombatStats,
    pub victim: CombatStats,
    pub rolls: DamageRolls,
    pub refine_bonus: i32,
    pub party_attacker_bonus: i32,
    pub attack_bonus_pct: i32,
    pub melee_magic_attack_bonus_pct: i32,
    pub defence_bonus_pct: i32,
    pub damage_multiplier: f32,
    pub ignore_defence: bool,
    pub ignore_target_rating: bool,
}

impl NormalHitInput {
    pub fn unmodified(attacker: CombatStats, victim: CombatStats, rolls: DamageRolls) -> Self {
        Self {
            attacker,
            victim,
            rolls,
            refine_bonus: 0,
            party_attacker_bonus: 0,
            attack_bonus_pct: 0,
            melee_magic_attack_bonus_pct: 0,
            defence_bonus_pct: 0,
            damage_multiplier: 1.0,
            ignore_defence: false,
            ignore_target_rating: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalHitOutcome {
    pub attack_rating_per_mille: i32,
    pub attack_value: i32,
    pub defence_value: i32,
    pub damage: i32,
}

pub fn attack_rating(
    attacker_level: i32,
    attacker_dx: i32,
    victim_dx: i32,
    ignore_target_rating: bool,
) -> f32 {
    let attack_rating_source = ((attacker_dx * 4 + attacker_level * 2) / 6).min(90);

    let evasion_rating_source = ((victim_dx * 4 + attacker_level * 2) / 6).min(90);

    let attack_rating = (attack_rating_source as f32 + 210.0) / 300.0;
    if ignore_target_rating {
        return attack_rating;
    }

    let evasion_rating =
        ((evasion_rating_source * 2 + 5) as f32 / (evasion_rating_source + 95) as f32) * 0.3;
    attack_rating - evasion_rating
}

pub fn normal_hit_damage(input: NormalHitInput) -> NormalHitOutcome {
    let attack_rating = attack_rating_for_hit(input);
    let attack_value = melee_attack_value(input, attack_rating);
    let defence_value = effective_defence(input);
    let raw_damage = (attack_value - defence_value).max(0);
    let damage = battle_damage_floor(raw_damage, input.rolls.low_damage_fallback);

    NormalHitOutcome {
        attack_rating_per_mille: (attack_rating * 1000.0) as i32,
        attack_value,
        defence_value,
        damage,
    }
}

pub fn attack_rating_for_hit(input: NormalHitInput) -> f32 {
    attack_rating(
        input.attacker.level,
        input.attacker.dx,
        input.victim.dx,
        input.ignore_target_rating,
    )
}

pub fn melee_attack_value(input: NormalHitInput, attack_rating: f32) -> i32 {
    let rolled_damage = input.rolls.weapon_or_mob_damage.max(0) * 2;
    let level_attack = input.attacker.level * 2;
    let mut attack_value = input.attacker.attack_grade + rolled_damage - level_attack;
    attack_value = (attack_value as f32 * attack_rating) as i32;
    attack_value += level_attack;
    attack_value += input.refine_bonus * 2;
    attack_value += input.party_attacker_bonus;
    attack_value =
        attack_value * (100 + input.attack_bonus_pct + input.melee_magic_attack_bonus_pct) / 100;

    if input.damage_multiplier.is_finite() {
        attack_value = (attack_value as f32 * input.damage_multiplier) as i32;
    }

    attack_value
}

pub fn effective_defence(input: NormalHitInput) -> i32 {
    if input.ignore_defence {
        0
    } else {
        input.victim.defence_grade * (100 + input.defence_bonus_pct) / 100
    }
}

pub fn battle_damage_floor(damage: i32, low_damage_fallback: i32) -> i32 {
    if damage < 3 {
        low_damage_fallback.clamp(1, 5)
    } else {
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_rating_preserves_legacy_attacker_level_for_evasion_source() {
        let rating = attack_rating(35, 18, 12, false);

        assert_eq!((rating * 1000.0) as i32, 663);
    }

    #[test]
    fn normal_hit_pipeline_subtracts_defence_after_attack_grade_and_roll() {
        let outcome = normal_hit_damage(NormalHitInput::unmodified(
            CombatStats {
                level: 10,
                dx: 6,
                attack_grade: 44,
                defence_grade: 0,
            },
            CombatStats {
                level: 8,
                dx: 5,
                attack_grade: 0,
                defence_grade: 18,
            },
            DamageRolls {
                weapon_or_mob_damage: 12,
                low_damage_fallback: 4,
            },
        ));

        assert_eq!(outcome.attack_value, 52);
        assert_eq!(outcome.defence_value, 18);
        assert_eq!(outcome.damage, 34);
    }

    #[test]
    fn very_low_damage_uses_caller_supplied_legacy_fallback_roll() {
        let outcome = normal_hit_damage(NormalHitInput::unmodified(
            CombatStats {
                level: 1,
                dx: 1,
                attack_grade: 1,
                defence_grade: 0,
            },
            CombatStats {
                level: 1,
                dx: 90,
                attack_grade: 0,
                defence_grade: 99,
            },
            DamageRolls {
                weapon_or_mob_damage: 0,
                low_damage_fallback: 5,
            },
        ));

        assert_eq!(outcome.damage, 5);
    }
}
