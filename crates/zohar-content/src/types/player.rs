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
