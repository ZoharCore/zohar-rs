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
