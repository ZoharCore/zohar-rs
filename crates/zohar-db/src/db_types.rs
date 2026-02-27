use strum_macros::{AsRefStr, EnumString};
use zohar_domain::Empire as DomainEmpire;
use zohar_domain::entity::player::PlayerBaseAppearance as DomainAppearanceVariant;
use zohar_domain::entity::player::PlayerClass as DomainPlayerClass;
use zohar_domain::entity::player::PlayerGender as DomainPlayerGender;

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
pub(crate) enum DbEmpire {
    #[strum(serialize = "RED")]
    Red,
    #[strum(serialize = "YELLOW")]
    Yellow,
    #[strum(serialize = "BLUE")]
    Blue,
}

impl From<DbEmpire> for DomainEmpire {
    fn from(value: DbEmpire) -> Self {
        match value {
            DbEmpire::Red => DomainEmpire::Red,
            DbEmpire::Yellow => DomainEmpire::Yellow,
            DbEmpire::Blue => DomainEmpire::Blue,
        }
    }
}

impl From<DomainEmpire> for DbEmpire {
    fn from(value: DomainEmpire) -> Self {
        match value {
            DomainEmpire::Red => DbEmpire::Red,
            DomainEmpire::Yellow => DbEmpire::Yellow,
            DomainEmpire::Blue => DbEmpire::Blue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
pub(crate) enum DbPlayerClass {
    #[strum(serialize = "WARRIOR")]
    Warrior,
    #[strum(serialize = "NINJA")]
    Ninja,
    #[strum(serialize = "SURA")]
    Sura,
    #[strum(serialize = "SHAMAN")]
    Shaman,
}

impl From<DbPlayerClass> for DomainPlayerClass {
    fn from(value: DbPlayerClass) -> Self {
        match value {
            DbPlayerClass::Warrior => DomainPlayerClass::Warrior,
            DbPlayerClass::Ninja => DomainPlayerClass::Ninja,
            DbPlayerClass::Sura => DomainPlayerClass::Sura,
            DbPlayerClass::Shaman => DomainPlayerClass::Shaman,
        }
    }
}

impl From<DomainPlayerClass> for DbPlayerClass {
    fn from(value: DomainPlayerClass) -> Self {
        match value {
            DomainPlayerClass::Warrior => DbPlayerClass::Warrior,
            DomainPlayerClass::Ninja => DbPlayerClass::Ninja,
            DomainPlayerClass::Sura => DbPlayerClass::Sura,
            DomainPlayerClass::Shaman => DbPlayerClass::Shaman,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
pub(crate) enum DbPlayerGender {
    #[strum(serialize = "M")]
    Male,
    #[strum(serialize = "F")]
    Female,
}

impl From<DbPlayerGender> for DomainPlayerGender {
    fn from(value: DbPlayerGender) -> Self {
        match value {
            DbPlayerGender::Male => DomainPlayerGender::Male,
            DbPlayerGender::Female => DomainPlayerGender::Female,
        }
    }
}

impl From<DomainPlayerGender> for DbPlayerGender {
    fn from(value: DomainPlayerGender) -> Self {
        match value {
            DomainPlayerGender::Male => DbPlayerGender::Male,
            DomainPlayerGender::Female => DbPlayerGender::Female,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
pub(crate) enum DbAppearance {
    #[strum(serialize = "A")]
    VariantA,
    #[strum(serialize = "B")]
    VariantB,
}

impl From<DbAppearance> for DomainAppearanceVariant {
    fn from(value: DbAppearance) -> Self {
        match value {
            DbAppearance::VariantA => DomainAppearanceVariant::VariantA,
            DbAppearance::VariantB => DomainAppearanceVariant::VariantB,
        }
    }
}

impl From<DomainAppearanceVariant> for DbAppearance {
    fn from(value: DomainAppearanceVariant) -> Self {
        match value {
            DomainAppearanceVariant::VariantA => DbAppearance::VariantA,
            DomainAppearanceVariant::VariantB => DbAppearance::VariantB,
        }
    }
}
