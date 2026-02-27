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

#[derive(Debug, Clone)]
pub struct ContentMob {
    pub mob_id: i64,
    pub code: String,
    pub name: String,
    pub mob_type: MobType,
    pub rank: MobRank,
    pub level: i64,
    pub move_speed: i64,
    pub attack_speed: i64,
}
