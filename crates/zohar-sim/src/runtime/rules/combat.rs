use zohar_domain::entity::mob::MobBattleType;

pub(crate) const PLAYER_MELEE_REACH_M: f32 = 1.5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AttackTiming {
    pub(crate) cooldown_ms: u64,
    pub(crate) packet_duration_ms: u32,
}

pub(crate) fn effective_attack_range_m(range: u16, _battle_type: MobBattleType) -> f32 {
    (range as f32 / 100.0).max(1.5)
}

pub(crate) fn attack_timing_for_mob(attack_speed: u8) -> AttackTiming {
    let speed = u32::from(attack_speed.max(1));
    let cooldown_ms = (120_000 / speed).clamp(400, 2_000);
    let packet_duration_ms = (cooldown_ms / 2).clamp(200, 1_000);
    AttackTiming {
        cooldown_ms: u64::from(cooldown_ms),
        packet_duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_timing_is_clamped_to_expected_bounds() {
        let slow = attack_timing_for_mob(1);
        let fast = attack_timing_for_mob(250);

        assert_eq!(slow.cooldown_ms, 2_000);
        assert_eq!(slow.packet_duration_ms, 1_000);
        assert_eq!(fast.cooldown_ms, 480);
        assert_eq!(fast.packet_duration_ms, 240);
    }
}
