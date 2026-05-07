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
        flags: FloatingDamageFlags,
        damage: i32,
    },

    #[brw(magic = 0x46_u8)]
    TriggerProjectileEffect {
        kind: ProjectileKind,
        from_entity: game_pkt::NetId,
        to_entity: game_pkt::NetId,
    },

    #[brw(magic = 0x72_u8)]
    TriggerSpecialEffect {
        kind: SpecialEffectKind,
        target: game_pkt::NetId,
    },
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
pub enum ProjectileKind {
    Experience = 1,
    HpMedium = 2,
    HpLarge = 3,
    SpSmall = 4,
    SpMedium = 5,
    SpLarge = 6,
    FireworkA = 7,
    FireworkB = 8,
    FireworkC = 9,
    FireworkD = 10,
    FireworkE = 11,
    FireworkF = 12,
    FireworkG = 13,
    LightningClaw = 14,
    HpSmall = 15,
    FlameSpirit = 16,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FloatingDamageFlags: u8 {
        const NORMAL = 1 << 0;
        const POISON = 1 << 1;
        const DODGE = 1 << 2;
        const BLOCK = 1 << 3;
        const PENETRATE = 1 << 4;
        const CRITICAL = 1 << 5;
    }
}

game_pkt::impl_bitflags_binrw!(FloatingDamageFlags, u8);

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
pub enum SpecialEffectKind {
    RedSurge = 1,
    BlueSurge = 2,
    GreenSurge = 3,
    PurpleSurge = 4,

    CriticalStrike = 5,
    PiercingStrike = 6,
    BlockedStrike = 7,
    DodgedStrike = 8,

    RedSurgeAlt = 19,
    BlueSurgeAlt = 20,
}
