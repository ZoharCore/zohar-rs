use strum::EnumString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
pub enum PlayerClass {
    #[strum(serialize = "WARRIOR")]
    Warrior,
    #[strum(serialize = "NINJA")]
    Ninja,
    #[strum(serialize = "SURA")]
    Sura,
    #[strum(serialize = "SHAMAN")]
    Shaman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
pub enum Gender {
    #[strum(serialize = "MALE")]
    Male,
    #[strum(serialize = "FEMALE")]
    Female,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerClassBaseStats {
    pub player_class: PlayerClass,
    pub base_strength: i32,
    pub base_vitality: i32,
    pub base_dexterity: i32,
    pub base_intelligence: i32,
    pub base_hp: i32,
    pub base_sp: i32,
    pub hp_per_vitality: i32,
    pub sp_per_intelligence: i32,
    pub hp_per_level_min: i32,
    pub hp_per_level_max: i32,
    pub sp_per_level_min: i32,
    pub sp_per_level_max: i32,
    pub base_stamina: i32,
    pub stamina_per_vitality: i32,
    pub stamina_per_level_min: i32,
    pub stamina_per_level_max: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelExp {
    pub level: i32,
    pub next_exp: i64,
    pub death_loss_pct: i32,
}
