use zohar_domain::entity::mob::MobBattleType;
use zohar_map_port::PacketDuration;

use crate::runtime::time::SimDuration;

pub(crate) const PLAYER_MELEE_REACH_M: f32 = 1.5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AttackTiming {
    pub(crate) cooldown: SimDuration,
    pub(crate) packet_duration: PacketDuration,
}

pub(crate) fn effective_attack_range_m(range: u16, _battle_type: MobBattleType) -> f32 {
    (range as f32 / 100.0).max(1.5)
}

pub(crate) fn attack_timing_for_mob(attack_speed: u8) -> AttackTiming {
    let speed = u32::from(attack_speed.max(1));
    let cooldown_ms = (120_000 / speed).clamp(400, 2_000);
    let packet_duration_ms = (cooldown_ms / 2).clamp(200, 1_000);
    AttackTiming {
        cooldown: SimDuration::from_millis(u64::from(cooldown_ms)),
        packet_duration: PacketDuration::new(packet_duration_ms),
    }
}
