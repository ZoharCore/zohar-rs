use crate::game_pkt;
use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum CombatC2s {
    #[brw(magic = 0x02_u8)]
    InputAttack {
        attack_type: game_pkt::ZeroOpt<super::Skill>,
        target: game_pkt::NetId,
        _unknown: u16,
    },
    #[brw(magic = 0x3D_u8)]
    SignalTargetSwitch { target: game_pkt::NetId },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum CombatS2c {
    #[brw(magic = 0x0D_u8)]
    SetEntityStunned { target: game_pkt::NetId },

    #[brw(magic = 0x0E_u8)]
    SetEntityDead { target: game_pkt::NetId },

    #[brw(magic = 0x3F_u8)]
    SyncEntityHealthBar { target: game_pkt::NetId, hp_pct: u8 },

    #[brw(magic = 0x87_u8)]
    TriggerFloatingDamage {
        target: game_pkt::NetId,
        flags: u8,
        damage: i32,
    },
}
