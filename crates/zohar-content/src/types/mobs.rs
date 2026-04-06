bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MobAiFlags: u32 {
        const AGGR = 1 << 0;
        const COWARD = 1 << 1;
        const BERSERK = 1 << 2;
        const STONESKIN = 1 << 3;
        const GODSPEED = 1 << 4;
        const DEATHBLOW = 1 << 5;
        const REVIVE = 1 << 6;
        const NOMOVE = 1 << 7;
        const NOATTSHINSU = 1 << 8;
        const NOATTCHUNJO = 1 << 9;
        const NOATTJINNO = 1 << 10;
        const ATTMOB = 1 << 11;
    }
}

impl std::str::FromStr for MobAiFlags {
    type Err = bitflags::parser::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        bitflags::parser::from_str::<Self>(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum MobType {
    #[strum(serialize = "NPC")]
    Npc,
    #[strum(serialize = "MONSTER")]
    Monster,
    #[strum(serialize = "STONE")]
    Stone,
    #[strum(serialize = "WARP")]
    Warp,
    #[strum(serialize = "DOOR")]
    Door,
    #[strum(serialize = "BUILDING")]
    Building,
    #[strum(serialize = "PC")]
    Pc,
    #[strum(serialize = "POLYMORPH_PC")]
    PolymorphPc,
    #[strum(serialize = "HORSE")]
    Horse,
    #[strum(serialize = "GOTO")]
    Goto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display)]
pub enum MobRank {
    #[strum(serialize = "PAWN")]
    Pawn,
    #[strum(serialize = "S_PAWN")]
    SuperPawn,
    #[strum(serialize = "KNIGHT")]
    Knight,
    #[strum(serialize = "S_KNIGHT")]
    SuperKnight,
    #[strum(serialize = "BOSS")]
    Boss,
    #[strum(serialize = "KING")]
    King,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display)]
pub enum MobBattleType {
    #[strum(serialize = "MELEE")]
    Melee,
    #[strum(serialize = "RANGE")]
    Range,
    #[strum(serialize = "MAGIC")]
    Magic,
    #[strum(serialize = "SPECIAL")]
    Special,
    #[strum(serialize = "POWER")]
    Power,
    #[strum(serialize = "TANKER")]
    Tanker,
    #[strum(serialize = "SUPER_POWER")]
    SuperPower,
    #[strum(serialize = "SUPER_TANKER")]
    SuperTanker,
}

#[derive(Debug, Clone)]
pub struct ContentMob {
    pub mob_id: i64,
    pub code: String,
    pub name: String,
    pub mob_type: MobType,
    pub rank: MobRank,
    pub battle_type: MobBattleType,
    pub level: i64,
    pub ai_flags: MobAiFlags,
    pub move_speed: i64,
    pub attack_speed: i64,
    pub aggressive_sight: i64,
    pub attack_range: i64,
}
