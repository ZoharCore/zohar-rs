use binrw::binrw;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::game_pkt;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum StatsS2c {
    #[brw(magic = 0x10_u8)]
    SetMainCharacterStats { stats: WireStatSnapshot },

    #[brw(magic = 0x11_u8)]
    SetEntityStat {
        #[brw(pad_before = 3)]
        net_id: game_pkt::NetId,
        stat_id: WireStatPoint,
        delta: i32,
        absolute: i32,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireStatSnapshot {
    pub stats: [u32; Self::STATS_COUNT],
}

impl Default for WireStatSnapshot {
    fn default() -> Self {
        Self {
            stats: [0; Self::STATS_COUNT],
        }
    }
}

impl WireStatSnapshot {
    const STATS_COUNT: usize = 255;

    pub fn get(&self, stat: WireStatPoint) -> u32 {
        self.stats[u8::from(stat) as usize]
    }

    pub fn set(&mut self, stat: WireStatPoint, value: u32) {
        self.stats[u8::from(stat) as usize] = value;
    }
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoPrimitive, TryFromPrimitive)]
pub enum WireStatPoint {
    Level = 1,
    Voice = 2,
    Exp = 3,
    NextExp = 4,
    Hp = 5,
    MaxHp = 6,
    Sp = 7,
    MaxSp = 8,
    Stamina = 9,
    MaxStamina = 10,
    Gold = 11,
    St = 12,
    Ht = 13,
    Dx = 14,
    Iq = 15,
    DefGrade = 16,
    AttSpeed = 17,
    AttGrade = 18,
    MovSpeed = 19,
    ClientDefGrade = 20,
    CastingSpeed = 21,
    MagicAttGrade = 22,
    MagicDefGrade = 23,
    EmpirePoint = 24,
    LevelStep = 25,
    StatPoints = 26,
    SubSkillPoints = 27,
    SkillPoints = 28,
    WeaponMin = 29,
    WeaponMax = 30,
    HpRegen = 32,
    SpRegen = 33,
    BowDistance = 34,
    HpRecovery = 35,
    SpRecovery = 36,
    PoisonPct = 37,
    StunPct = 38,
    SlowPct = 39,
    CriticalPct = 40,
    PenetratePct = 41,
    CursePct = 42,
    AttBonusHuman = 43,
    AttBonusAnimal = 44,
    AttBonusOrc = 45,
    AttBonusMilgyo = 46,
    AttBonusUndead = 47,
    AttBonusDevil = 48,
    AttBonusInsect = 49,
    AttBonusFire = 50,
    AttBonusIce = 51,
    AttBonusDesert = 52,
    AttBonusMonster = 53,
    AttBonusWarrior = 54,
    AttBonusAssassin = 55,
    AttBonusSura = 56,
    AttBonusShaman = 57,
    AttBonusTree = 58,
    ResistWarrior = 59,
    ResistAssassin = 60,
    ResistSura = 61,
    ResistShaman = 62,
    StealHp = 63,
    StealSp = 64,
    ManaBurnPct = 65,
    DamageSpRecover = 66,
    Block = 67,
    Dodge = 68,
    ResistSword = 69,
    ResistTwoHand = 70,
    ResistDagger = 71,
    ResistBell = 72,
    ResistFan = 73,
    ResistBow = 74,
    ResistFire = 75,
    ResistElec = 76,
    ResistMagic = 77,
    ResistWind = 78,
    ReflectMelee = 79,
    ReflectCurse = 80,
    PoisonReduce = 81,
    KillSpRecover = 82,
    ExpDoubleBonus = 83,
    GoldDoubleBonus = 84,
    ItemDropBonus = 85,
    PotionBonus = 86,
    KillHpRecovery = 87,
    ImmuneStun = 88,
    ImmuneSlow = 89,
    ImmuneFall = 90,
    PartyAttackerBonus = 91,
    PartyTankerBonus = 92,
    AttBonus = 93,
    DefBonus = 94,
    AttGradeBonus = 95,
    DefGradeBonus = 96,
    MagicAttGradeBonus = 97,
    MagicDefGradeBonus = 98,
    ResistNormalDamage = 99,
    HitHpRecovery = 100,
    HitSpRecovery = 101,
    Manashield = 102,
    PartyBufferBonus = 103,
    PartySkillMasterBonus = 104,
    HpRecoverContinue = 105,
    SpRecoverContinue = 106,
    StealGold = 107,
    Polymorph = 108,
    Mount = 109,
    PartyHasteBonus = 110,
    PartyDefenderBonus = 111,
    StatResetCount = 112,
    HorseSkillPoints = 113,
    MallAttBonus = 114,
    MallDefBonus = 115,
    MallExpBonus = 116,
    MallItemBonus = 117,
    MallGoldBonus = 118,
    MaxHpPct = 119,
    MaxSpPct = 120,
    SkillDamageBonus = 121,
    NormalHitDamageBonus = 122,
    SkillDefendBonus = 123,
    NormalHitDefendBonus = 124,
    PcBangExpBonus = 125,
    PcBangDropBonus = 126,
    RamadanCandyBonusExp = 127,
    Energy = 128,
    EnergyEndTime = 129,
    CostumeAttrBonus = 130,
    MagicAttBonusPer = 131,
    MeleeMagicAttBonusPer = 132,
    ResistIce = 133,
    ResistEarth = 134,
    ResistDark = 135,
    ResistCritical = 136,
    ResistPenetrate = 137,
}
