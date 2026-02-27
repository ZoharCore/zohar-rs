#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum MotionEntityKind {
    #[strum(serialize = "MOB")]
    Mob,
    #[strum(serialize = "PLAYER")]
    Player,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum MotionMode {
    #[strum(serialize = "GENERAL")]
    General,
    #[strum(serialize = "TWOHAND_SWORD")]
    TwohandSword,
    #[strum(serialize = "ONEHAND_SWORD")]
    OnehandSword,
    #[strum(serialize = "DUALHAND_SWORD")]
    DualhandSword,
    #[strum(serialize = "BOW")]
    Bow,
    #[strum(serialize = "BELL")]
    Bell,
    #[strum(serialize = "FAN")]
    Fan,
    #[strum(serialize = "FISHING")]
    Fishing,
    #[strum(serialize = "HORSE")]
    Horse,
    #[strum(serialize = "HORSE_ONEHAND_SWORD")]
    HorseOnehandSword,
    #[strum(serialize = "HORSE_TWOHAND_SWORD")]
    HorseTwohandSword,
    #[strum(serialize = "HORSE_DUALHAND_SWORD")]
    HorseDualhandSword,
    #[strum(serialize = "HORSE_BOW")]
    HorseBow,
    #[strum(serialize = "HORSE_FAN")]
    HorseFan,
    #[strum(serialize = "HORSE_BELL")]
    HorseBell,
    #[strum(serialize = "WEDDING_DRESS")]
    WeddingDress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum MotionAction {
    #[strum(serialize = "WAIT")]
    Wait,
    #[strum(serialize = "WALK")]
    Walk,
    #[strum(serialize = "RUN")]
    Run,
    #[strum(serialize = "DAMAGE")]
    Damage,
    #[strum(serialize = "DAMAGE_FLYING")]
    DamageFlying,
    #[strum(serialize = "STAND_UP")]
    StandUp,
    #[strum(serialize = "DAMAGE_BACK")]
    DamageBack,
    #[strum(serialize = "DAMAGE_FLYING_BACK")]
    DamageFlyingBack,
    #[strum(serialize = "STAND_UP_BACK")]
    StandUpBack,
    #[strum(serialize = "DEAD")]
    Dead,
    #[strum(serialize = "DEAD_BACK")]
    DeadBack,
    #[strum(serialize = "NORMAL_ATTACK")]
    NormalAttack,
    #[strum(serialize = "COMBO_ATTACK_1")]
    ComboAttack1,
    #[strum(serialize = "COMBO_ATTACK_2")]
    ComboAttack2,
    #[strum(serialize = "COMBO_ATTACK_3")]
    ComboAttack3,
    #[strum(serialize = "SPAWN")]
    Spawn,
    #[strum(serialize = "STOP")]
    Stop,
    #[strum(serialize = "SPECIAL_1")]
    Special1,
    #[strum(serialize = "SPECIAL_2")]
    Special2,
    #[strum(serialize = "SPECIAL_3")]
    Special3,
    #[strum(serialize = "SPECIAL_4")]
    Special4,
    #[strum(serialize = "SPECIAL_5")]
    Special5,
    #[strum(serialize = "SPECIAL_6")]
    Special6,
    #[strum(serialize = "SKILL_1")]
    Skill1,
    #[strum(serialize = "SKILL_2")]
    Skill2,
    #[strum(serialize = "SKILL_3")]
    Skill3,
    #[strum(serialize = "SKILL_4")]
    Skill4,
    #[strum(serialize = "SKILL_5")]
    Skill5,
}

#[derive(Debug, Clone)]
pub struct PlayerMotionProfile {
    pub profile_id: i64,
    pub legacy_race_num: i64,
    pub player_class: super::player::PlayerClass,
    pub gender: super::player::Gender,
}

#[derive(Debug, Clone)]
pub struct ContentMotion {
    pub motion_id: i64,
    pub motion_entity_id: i64,
    pub entity_kind: MotionEntityKind,
    pub mob_id: Option<i64>,
    pub player_profile_id: Option<i64>,
    pub motion_mode: MotionMode,
    pub motion_action: MotionAction,
    pub duration_ms: i64,
    pub accum_x: Option<f64>,
    pub accum_y: Option<f64>,
    pub source: String,
}
