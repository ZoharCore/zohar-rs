use zohar_domain::entity::mob::MobBattleType;
use zohar_map_port::PacketDuration;

use crate::runtime::time::SimDuration;

pub(crate) const PLAYER_MELEE_REACH_M: f32 = 1.5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AttackTiming {
    pub(crate) attack_gate: SimDuration,
    pub(crate) fallback_action_duration: PacketDuration,
}

pub(crate) fn effective_attack_range_m(range: u16, _battle_type: MobBattleType) -> f32 {
    (range as f32 / 100.0).max(1.5)
}

pub(crate) fn attack_timing_for_mob(attack_speed: u8) -> AttackTiming {
    let attack_gate_ms = speed_scaled_action_duration_ms(u16::from(attack_speed), 2_000);
    AttackTiming {
        attack_gate: SimDuration::from_millis(u64::from(attack_gate_ms)),
        fallback_action_duration: PacketDuration::new(2_000),
    }
}

fn speed_scaled_action_duration_ms(speed: u16, base_ms: u32) -> u32 {
    let speed = u32::from(speed).max(1);
    let duration_percent = match speed.cmp(&100) {
        std::cmp::Ordering::Less => 200 - speed,
        std::cmp::Ordering::Equal => 100,
        std::cmp::Ordering::Greater => 10_000 / speed,
    };

    base_ms.saturating_mul(duration_percent) / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mob_attack_gate_uses_compatible_speed_scaled_duration() {
        assert_eq!(
            attack_timing_for_mob(60).attack_gate,
            SimDuration::from_millis(2_800)
        );
        assert_eq!(
            attack_timing_for_mob(100).attack_gate,
            SimDuration::from_millis(2_000)
        );
        assert_eq!(
            attack_timing_for_mob(200).attack_gate,
            SimDuration::from_millis(1_000)
        );
    }

    #[test]
    fn mob_attack_fallback_action_duration_matches_legacy_default_motion_duration() {
        assert_eq!(
            attack_timing_for_mob(60).fallback_action_duration,
            PacketDuration::new(2_000)
        );
    }
}
