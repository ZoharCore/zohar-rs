use super::Stat;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatRole {
    Persistent,
    RuntimeResource,
    Computed,
    ModifierAccumulator,
    RuntimeIdentity,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatContributionKind {
    AdditiveScalar,
    CappedPercentage,
    FlagCounter,
}

#[allow(dead_code)]
pub(crate) trait StatExt {
    fn role(self) -> StatRole;
    fn contribution_kind(self) -> Option<StatContributionKind>;
    fn accepts_source_contribution(self) -> bool;
}

impl StatExt for Stat {
    fn role(self) -> StatRole {
        match self {
            Self::Level
            | Self::Exp
            | Self::NextExp
            | Self::Gold
            | Self::St
            | Self::Ht
            | Self::Dx
            | Self::Iq
            | Self::StatPoints
            | Self::StatResetCount => StatRole::Persistent,

            Self::Hp | Self::Sp | Self::Stamina | Self::HpRecovery | Self::SpRecovery => {
                StatRole::RuntimeResource
            }

            Self::MaxHp
            | Self::MaxSp
            | Self::MaxStamina
            | Self::LevelStep
            | Self::DefGrade
            | Self::AttSpeed
            | Self::AttGrade
            | Self::MovSpeed
            | Self::DisplayedDefGrade
            | Self::CastingSpeed
            | Self::MagicAttGrade
            | Self::MagicDefGrade => StatRole::Computed,

            Self::BonusMaxHp
            | Self::BonusMaxSp
            | Self::BonusMaxStamina
            | Self::ArmorDefence
            | Self::MaxHpPrePctBonus
            | Self::ImmuneStun
            | Self::ImmuneSlow
            | Self::ImmuneFall
            | Self::PartyTankerBonus
            | Self::AttGradeBonus
            | Self::DefGradeBonus
            | Self::MagicAttGradeBonus
            | Self::MagicDefGradeBonus
            | Self::PartySkillMasterBonus
            | Self::PartyHasteBonus
            | Self::PartyDefenderBonus
            | Self::MaxHpPct
            | Self::MaxSpPct => StatRole::ModifierAccumulator,

            Self::Polymorph | Self::Mount => StatRole::RuntimeIdentity,
        }
    }

    fn contribution_kind(self) -> Option<StatContributionKind> {
        match self {
            Self::MaxHpPct | Self::MaxSpPct => Some(StatContributionKind::CappedPercentage),

            Self::ImmuneStun | Self::ImmuneSlow | Self::ImmuneFall => {
                Some(StatContributionKind::FlagCounter)
            }

            Self::St
            | Self::Ht
            | Self::Dx
            | Self::Iq
            | Self::DefGrade
            | Self::AttSpeed
            | Self::AttGrade
            | Self::MovSpeed
            | Self::CastingSpeed
            | Self::MagicAttGrade
            | Self::MagicDefGrade
            | Self::BonusMaxHp
            | Self::BonusMaxSp
            | Self::BonusMaxStamina
            | Self::ArmorDefence
            | Self::MaxHpPrePctBonus
            | Self::PartyTankerBonus
            | Self::AttGradeBonus
            | Self::DefGradeBonus
            | Self::MagicAttGradeBonus
            | Self::MagicDefGradeBonus
            | Self::PartySkillMasterBonus
            | Self::PartyHasteBonus
            | Self::PartyDefenderBonus => Some(StatContributionKind::AdditiveScalar),

            _ => None,
        }
    }

    fn accepts_source_contribution(self) -> bool {
        self.contribution_kind().is_some()
    }
}
