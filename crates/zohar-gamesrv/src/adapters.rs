use tracing::warn;
use zohar_db::PlayerSummaryRow;
use zohar_domain::Empire as DomainEmpire;
use zohar_domain::appearance::EntityKind;
use zohar_domain::entity::mob::MobKind;
use zohar_domain::entity::player::skill::{
    NinjaSkillBranch, ShamanSkillBranch, SkillBranch as DomainSkillBranch,
    SkillId as DomainSkillId, SuraSkillBranch, WarriorSkillBranch,
};
use zohar_domain::entity::player::{
    CoreStatAllocations, PlayerBaseAppearance as DomainPlayerAppearance,
    PlayerClass as DomainPlayerClass, PlayerGender as DomainPlayerGender, PlayerPlaytime,
    PlayerSlot, PlayerStats, PlayerSummary,
};
use zohar_domain::entity::{
    EntityId, MovementAnimation as DomainMovementAnimation, MovementKind as DomainMovementKind,
};
use zohar_domain::stat::Stat as DomainStat;
use zohar_gameplay::stats::game::{PlayerStatRules, StatSnapshot};
use zohar_map_port::{AttackIntent as PortAttackIntent, ChatChannel};
use zohar_protocol::game_pkt::ChatKind;
use zohar_protocol::game_pkt::ingame::Skill;
use zohar_protocol::game_pkt::ingame::movement::{
    MovementAnimation as ProtocolMovementAnimation, MovementKind,
};
use zohar_protocol::game_pkt::ingame::stats::{WireStatPoint, WireStatSnapshot};
use zohar_protocol::game_pkt::ingame::world::EntityType;
use zohar_protocol::game_pkt::select::{Player, PlayerBaseAppearance};
use zohar_protocol::game_pkt::{
    Empire, NetId, PlayerClassGendered, SkillBranch, WireServerAddr, ZeroOpt,
};

pub(crate) trait ToDomain<T> {
    fn to_domain(self) -> T;
}

pub(crate) trait ToProtocol<T> {
    fn to_protocol(self) -> T;
}

pub(crate) trait PlayerSummaryRowExt {
    fn to_domain_with_stats(&self, stats: PlayerSummaryStats<'_>) -> PlayerSummary;
}

#[derive(Clone, Copy)]
pub(crate) enum PlayerSummaryStats<'a> {
    Rules(&'a PlayerStatRules),
    CreateBase(&'a crate::PlayerCreateBaseStatTable),
}

impl PlayerSummaryStats<'_> {
    fn resolve_player_stats(
        self,
        class: DomainPlayerClass,
        allocations: zohar_db::PlayerCoreStatAllocationRow,
    ) -> Option<PlayerStats> {
        match self {
            Self::Rules(rules) => rules.resolve_player_stats(
                class,
                CoreStatAllocations {
                    allocated_str: allocations.allocated_str,
                    allocated_vit: allocations.allocated_vit,
                    allocated_dex: allocations.allocated_dex,
                    allocated_int: allocations.allocated_int,
                },
            ),
            Self::CreateBase(base_stats) => base_stats.resolve_player_stats(class, allocations),
        }
    }
}

impl ToDomain<EntityId> for NetId {
    fn to_domain(self) -> EntityId {
        self.0.into()
    }
}

impl ToProtocol<NetId> for EntityId {
    fn to_protocol(self) -> NetId {
        self.0.into()
    }
}

impl ToDomain<DomainMovementKind> for MovementKind {
    fn to_domain(self) -> DomainMovementKind {
        match self {
            MovementKind::Wait => DomainMovementKind::Wait,
            MovementKind::Move => DomainMovementKind::Move,
            MovementKind::Attack => DomainMovementKind::Attack,
            MovementKind::Combo => DomainMovementKind::Combo,
        }
    }
}

impl ToProtocol<MovementKind> for DomainMovementKind {
    fn to_protocol(self) -> MovementKind {
        match self {
            DomainMovementKind::Wait => MovementKind::Wait,
            DomainMovementKind::Move => MovementKind::Move,
            DomainMovementKind::Attack => MovementKind::Attack,
            DomainMovementKind::Combo => MovementKind::Combo,
        }
    }
}

impl ToDomain<DomainMovementAnimation> for ProtocolMovementAnimation {
    fn to_domain(self) -> DomainMovementAnimation {
        match self {
            ProtocolMovementAnimation::Run => DomainMovementAnimation::Run,
            ProtocolMovementAnimation::Walk => DomainMovementAnimation::Walk,
        }
    }
}

impl ToProtocol<ProtocolMovementAnimation> for DomainMovementAnimation {
    fn to_protocol(self) -> ProtocolMovementAnimation {
        match self {
            DomainMovementAnimation::Run => ProtocolMovementAnimation::Run,
            DomainMovementAnimation::Walk => ProtocolMovementAnimation::Walk,
        }
    }
}

impl ToDomain<ChatChannel> for ChatKind {
    fn to_domain(self) -> ChatChannel {
        match self {
            ChatKind::Speak => ChatChannel::Speak,
            ChatKind::Info => ChatChannel::Info,
            ChatKind::Notice => ChatChannel::Notice,
            ChatKind::Command => ChatChannel::Command,
            ChatKind::Shout => ChatChannel::Shout,
        }
    }
}

impl ToProtocol<ChatKind> for ChatChannel {
    fn to_protocol(self) -> ChatKind {
        match self {
            ChatChannel::Speak => ChatKind::Speak,
            ChatChannel::Info => ChatKind::Info,
            ChatChannel::Notice => ChatKind::Notice,
            ChatChannel::Command => ChatKind::Command,
            ChatChannel::Shout => ChatKind::Shout,
        }
    }
}

impl ToDomain<DomainPlayerAppearance> for PlayerBaseAppearance {
    fn to_domain(self) -> DomainPlayerAppearance {
        match self {
            PlayerBaseAppearance::VariantA => DomainPlayerAppearance::VariantA,
            PlayerBaseAppearance::VariantB => DomainPlayerAppearance::VariantB,
        }
    }
}

impl ToProtocol<PlayerBaseAppearance> for DomainPlayerAppearance {
    fn to_protocol(self) -> PlayerBaseAppearance {
        match self {
            DomainPlayerAppearance::VariantA => PlayerBaseAppearance::VariantA,
            DomainPlayerAppearance::VariantB => PlayerBaseAppearance::VariantB,
        }
    }
}

impl ToDomain<(DomainPlayerClass, DomainPlayerGender)> for PlayerClassGendered {
    fn to_domain(self) -> (DomainPlayerClass, DomainPlayerGender) {
        match self {
            PlayerClassGendered::WarriorMale => {
                (DomainPlayerClass::Warrior, DomainPlayerGender::Male)
            }
            PlayerClassGendered::WarriorFemale => {
                (DomainPlayerClass::Warrior, DomainPlayerGender::Female)
            }
            PlayerClassGendered::NinjaMale => (DomainPlayerClass::Ninja, DomainPlayerGender::Male),
            PlayerClassGendered::NinjaFemale => {
                (DomainPlayerClass::Ninja, DomainPlayerGender::Female)
            }
            PlayerClassGendered::SuraMale => (DomainPlayerClass::Sura, DomainPlayerGender::Male),
            PlayerClassGendered::SuraFemale => {
                (DomainPlayerClass::Sura, DomainPlayerGender::Female)
            }
            PlayerClassGendered::ShamanMale => {
                (DomainPlayerClass::Shaman, DomainPlayerGender::Male)
            }
            PlayerClassGendered::ShamanFemale => {
                (DomainPlayerClass::Shaman, DomainPlayerGender::Female)
            }
        }
    }
}

impl ToProtocol<PlayerClassGendered> for (DomainPlayerClass, DomainPlayerGender) {
    fn to_protocol(self) -> PlayerClassGendered {
        match self {
            (DomainPlayerClass::Warrior, DomainPlayerGender::Male) => {
                PlayerClassGendered::WarriorMale
            }
            (DomainPlayerClass::Warrior, DomainPlayerGender::Female) => {
                PlayerClassGendered::WarriorFemale
            }
            (DomainPlayerClass::Ninja, DomainPlayerGender::Male) => PlayerClassGendered::NinjaMale,
            (DomainPlayerClass::Ninja, DomainPlayerGender::Female) => {
                PlayerClassGendered::NinjaFemale
            }
            (DomainPlayerClass::Sura, DomainPlayerGender::Male) => PlayerClassGendered::SuraMale,
            (DomainPlayerClass::Sura, DomainPlayerGender::Female) => {
                PlayerClassGendered::SuraFemale
            }
            (DomainPlayerClass::Shaman, DomainPlayerGender::Male) => {
                PlayerClassGendered::ShamanMale
            }
            (DomainPlayerClass::Shaman, DomainPlayerGender::Female) => {
                PlayerClassGendered::ShamanFemale
            }
        }
    }
}

impl ToDomain<DomainEmpire> for Empire {
    fn to_domain(self) -> DomainEmpire {
        match self {
            Empire::Red => DomainEmpire::Red,
            Empire::Yellow => DomainEmpire::Yellow,
            Empire::Blue => DomainEmpire::Blue,
        }
    }
}

impl ToProtocol<Empire> for DomainEmpire {
    fn to_protocol(self) -> Empire {
        match self {
            DomainEmpire::Red => Empire::Red,
            DomainEmpire::Yellow => Empire::Yellow,
            DomainEmpire::Blue => Empire::Blue,
        }
    }
}

impl PlayerSummaryRowExt for PlayerSummaryRow {
    fn to_domain_with_stats(&self, stats: PlayerSummaryStats<'_>) -> PlayerSummary {
        let slot = match self.slot {
            0 => PlayerSlot::First,
            1 => PlayerSlot::Second,
            2 => PlayerSlot::Third,
            3 => PlayerSlot::Fourth,
            other => {
                warn!(
                    slot = other,
                    "Invalid player slot from DB; defaulting to First"
                );
                PlayerSlot::First
            }
        };

        let stats = stats
            .resolve_player_stats(self.class, self.core_stat_allocations)
            .unwrap_or_else(|| {
                warn!(
                    class = ?self.class,
                    player_id = ?self.id,
                    "Missing class stats; defaulting visible stats to zero"
                );
                PlayerStats {
                    stat_str: 0,
                    stat_vit: 0,
                    stat_dex: 0,
                    stat_int: 0,
                }
            });

        PlayerSummary {
            id: self.id,
            slot,
            name: self.name.clone(),
            class: self.class,
            gender: self.gender,
            appearance: self.appearance,
            level: self.level,
            playtime: PlayerPlaytime::from_secs_i64(self.playtime_secs),
            stats,
        }
    }
}

pub(crate) trait ToProtocolPlayer {
    fn to_protocol_player(&self, endpoint: WireServerAddr) -> Player;
}

impl ToProtocol<SkillBranch> for DomainSkillBranch {
    fn to_protocol(self) -> SkillBranch {
        match self {
            DomainSkillBranch::Warrior(WarriorSkillBranch::Body)
            | DomainSkillBranch::Ninja(NinjaSkillBranch::BladeFight)
            | DomainSkillBranch::Sura(SuraSkillBranch::Weaponry)
            | DomainSkillBranch::Shaman(ShamanSkillBranch::Dragon) => SkillBranch::BranchA,

            DomainSkillBranch::Warrior(WarriorSkillBranch::Mental)
            | DomainSkillBranch::Ninja(NinjaSkillBranch::Archery)
            | DomainSkillBranch::Sura(SuraSkillBranch::BlackMagic)
            | DomainSkillBranch::Shaman(ShamanSkillBranch::Healing) => SkillBranch::BranchB,
        }
    }
}

impl ToDomain<DomainSkillId> for Skill {
    fn to_domain(self) -> DomainSkillId {
        match self {
            Skill::ThreeWayCut => DomainSkillId::ThreeWayCut,
            Skill::SwordSpin => DomainSkillId::SwordSpin,
            Skill::Berserk => DomainSkillId::Berserk,
            Skill::AuraOfTheSword => DomainSkillId::AuraOfTheSword,
            Skill::Dash => DomainSkillId::Dash,
            Skill::LifeForce => DomainSkillId::LifeForce,
            Skill::Shockwave => DomainSkillId::Shockwave,
            Skill::Bash => DomainSkillId::Bash,
            Skill::Stump => DomainSkillId::Stump,
            Skill::StrongBody => DomainSkillId::StrongBody,
            Skill::SwordStrike => DomainSkillId::SwordStrike,
            Skill::SwordOrb => DomainSkillId::SwordOrb,
            Skill::Ambush => DomainSkillId::Ambush,
            Skill::FastAttack => DomainSkillId::FastAttack,
            Skill::RollingDagger => DomainSkillId::RollingDagger,
            Skill::Stealth => DomainSkillId::Stealth,
            Skill::PoisonousCloud => DomainSkillId::PoisonousCloud,
            Skill::InsidiousPoison => DomainSkillId::InsidiousPoison,
            Skill::RepetitiveShot => DomainSkillId::RepetitiveShot,
            Skill::ArrowShower => DomainSkillId::ArrowShower,
            Skill::FireArrow => DomainSkillId::FireArrow,
            Skill::FeatherWalk => DomainSkillId::FeatherWalk,
            Skill::PoisonArrow => DomainSkillId::PoisonArrow,
            Skill::Spark => DomainSkillId::Spark,
            Skill::FingerStrike => DomainSkillId::FingerStrike,
            Skill::DragonSwirl => DomainSkillId::DragonSwirl,
            Skill::EnchantedBlade => DomainSkillId::EnchantedBlade,
            Skill::Fear => DomainSkillId::Fear,
            Skill::EnchantedArmor => DomainSkillId::EnchantedArmor,
            Skill::Dispel => DomainSkillId::Dispel,
            Skill::DarkStrike => DomainSkillId::DarkStrike,
            Skill::FlameStrike => DomainSkillId::FlameStrike,
            Skill::FlameSpirit => DomainSkillId::FlameSpirit,
            Skill::DarkProtection => DomainSkillId::DarkProtection,
            Skill::SpiritStrike => DomainSkillId::SpiritStrike,
            Skill::DarkOrb => DomainSkillId::DarkOrb,
            Skill::FlyingTalisman => DomainSkillId::FlyingTalisman,
            Skill::ShootingDragon => DomainSkillId::ShootingDragon,
            Skill::DragonRoar => DomainSkillId::DragonRoar,
            Skill::Blessing => DomainSkillId::Blessing,
            Skill::Reflect => DomainSkillId::Reflect,
            Skill::DragonAid => DomainSkillId::DragonAid,
            Skill::LightningThrow => DomainSkillId::LightningThrow,
            Skill::SummonLightning => DomainSkillId::SummonLightning,
            Skill::LightningClaw => DomainSkillId::LightningClaw,
            Skill::Cure => DomainSkillId::Cure,
            Skill::Swiftness => DomainSkillId::Swiftness,
            Skill::AttackUp => DomainSkillId::AttackUp,
            Skill::Leadership => DomainSkillId::Leadership,
            Skill::Combo => DomainSkillId::Combo,
            Skill::Fishing => DomainSkillId::Fishing,
            Skill::Mining => DomainSkillId::Mining,
            Skill::LanguageShinsoo => DomainSkillId::LanguageShinsoo,
            Skill::LanguageChunjo => DomainSkillId::LanguageChunjo,
            Skill::LanguageJinno => DomainSkillId::LanguageJinno,
            Skill::Polymorph => DomainSkillId::Polymorph,
            Skill::HorseRiding => DomainSkillId::HorseRiding,
            Skill::HorseSummon => DomainSkillId::HorseSummon,
            Skill::HorseWildAttack => DomainSkillId::HorseWildAttack,
            Skill::HorseCharge => DomainSkillId::HorseCharge,
            Skill::HorseEscape => DomainSkillId::HorseEscape,
            Skill::HorseWildAttackRange => DomainSkillId::HorseWildAttackRange,
            Skill::AddHp => DomainSkillId::AddHp,
            Skill::PenetrationResistance => DomainSkillId::PenetrationResistance,
            Skill::GuildEye => DomainSkillId::GuildEye,
            Skill::GuildBlood => DomainSkillId::GuildBlood,
            Skill::GuildBless => DomainSkillId::GuildBless,
            Skill::GuildSeonghwi => DomainSkillId::GuildSeonghwi,
            Skill::GuildAcceleration => DomainSkillId::GuildAcceleration,
            Skill::GuildBunno => DomainSkillId::GuildBunno,
            Skill::GuildJumun => DomainSkillId::GuildJumun,
            Skill::GuildTeleport => DomainSkillId::GuildTeleport,
            Skill::GuildDoor => DomainSkillId::GuildDoor,
        }
    }
}

impl ToProtocol<Skill> for DomainSkillId {
    fn to_protocol(self) -> Skill {
        match self {
            DomainSkillId::ThreeWayCut => Skill::ThreeWayCut,
            DomainSkillId::SwordSpin => Skill::SwordSpin,
            DomainSkillId::Berserk => Skill::Berserk,
            DomainSkillId::AuraOfTheSword => Skill::AuraOfTheSword,
            DomainSkillId::Dash => Skill::Dash,
            DomainSkillId::LifeForce => Skill::LifeForce,
            DomainSkillId::Shockwave => Skill::Shockwave,
            DomainSkillId::Bash => Skill::Bash,
            DomainSkillId::Stump => Skill::Stump,
            DomainSkillId::StrongBody => Skill::StrongBody,
            DomainSkillId::SwordStrike => Skill::SwordStrike,
            DomainSkillId::SwordOrb => Skill::SwordOrb,
            DomainSkillId::Ambush => Skill::Ambush,
            DomainSkillId::FastAttack => Skill::FastAttack,
            DomainSkillId::RollingDagger => Skill::RollingDagger,
            DomainSkillId::Stealth => Skill::Stealth,
            DomainSkillId::PoisonousCloud => Skill::PoisonousCloud,
            DomainSkillId::InsidiousPoison => Skill::InsidiousPoison,
            DomainSkillId::RepetitiveShot => Skill::RepetitiveShot,
            DomainSkillId::ArrowShower => Skill::ArrowShower,
            DomainSkillId::FireArrow => Skill::FireArrow,
            DomainSkillId::FeatherWalk => Skill::FeatherWalk,
            DomainSkillId::PoisonArrow => Skill::PoisonArrow,
            DomainSkillId::Spark => Skill::Spark,
            DomainSkillId::FingerStrike => Skill::FingerStrike,
            DomainSkillId::DragonSwirl => Skill::DragonSwirl,
            DomainSkillId::EnchantedBlade => Skill::EnchantedBlade,
            DomainSkillId::Fear => Skill::Fear,
            DomainSkillId::EnchantedArmor => Skill::EnchantedArmor,
            DomainSkillId::Dispel => Skill::Dispel,
            DomainSkillId::DarkStrike => Skill::DarkStrike,
            DomainSkillId::FlameStrike => Skill::FlameStrike,
            DomainSkillId::FlameSpirit => Skill::FlameSpirit,
            DomainSkillId::DarkProtection => Skill::DarkProtection,
            DomainSkillId::SpiritStrike => Skill::SpiritStrike,
            DomainSkillId::DarkOrb => Skill::DarkOrb,
            DomainSkillId::FlyingTalisman => Skill::FlyingTalisman,
            DomainSkillId::ShootingDragon => Skill::ShootingDragon,
            DomainSkillId::DragonRoar => Skill::DragonRoar,
            DomainSkillId::Blessing => Skill::Blessing,
            DomainSkillId::Reflect => Skill::Reflect,
            DomainSkillId::DragonAid => Skill::DragonAid,
            DomainSkillId::LightningThrow => Skill::LightningThrow,
            DomainSkillId::SummonLightning => Skill::SummonLightning,
            DomainSkillId::LightningClaw => Skill::LightningClaw,
            DomainSkillId::Cure => Skill::Cure,
            DomainSkillId::Swiftness => Skill::Swiftness,
            DomainSkillId::AttackUp => Skill::AttackUp,
            DomainSkillId::Leadership => Skill::Leadership,
            DomainSkillId::Combo => Skill::Combo,
            DomainSkillId::Fishing => Skill::Fishing,
            DomainSkillId::Mining => Skill::Mining,
            DomainSkillId::LanguageShinsoo => Skill::LanguageShinsoo,
            DomainSkillId::LanguageChunjo => Skill::LanguageChunjo,
            DomainSkillId::LanguageJinno => Skill::LanguageJinno,
            DomainSkillId::Polymorph => Skill::Polymorph,
            DomainSkillId::HorseRiding => Skill::HorseRiding,
            DomainSkillId::HorseSummon => Skill::HorseSummon,
            DomainSkillId::HorseWildAttack => Skill::HorseWildAttack,
            DomainSkillId::HorseCharge => Skill::HorseCharge,
            DomainSkillId::HorseEscape => Skill::HorseEscape,
            DomainSkillId::HorseWildAttackRange => Skill::HorseWildAttackRange,
            DomainSkillId::AddHp => Skill::AddHp,
            DomainSkillId::PenetrationResistance => Skill::PenetrationResistance,
            DomainSkillId::GuildEye => Skill::GuildEye,
            DomainSkillId::GuildBlood => Skill::GuildBlood,
            DomainSkillId::GuildBless => Skill::GuildBless,
            DomainSkillId::GuildSeonghwi => Skill::GuildSeonghwi,
            DomainSkillId::GuildAcceleration => Skill::GuildAcceleration,
            DomainSkillId::GuildBunno => Skill::GuildBunno,
            DomainSkillId::GuildJumun => Skill::GuildJumun,
            DomainSkillId::GuildTeleport => Skill::GuildTeleport,
            DomainSkillId::GuildDoor => Skill::GuildDoor,
        }
    }
}

impl ToProtocol<u8> for DomainSkillId {
    fn to_protocol(self) -> u8 {
        let skill: Skill = <DomainSkillId as ToProtocol<Skill>>::to_protocol(self);
        skill.into()
    }
}

impl ToProtocol<u8> for PortAttackIntent {
    fn to_protocol(self) -> u8 {
        match self {
            PortAttackIntent::Basic => 0,
            PortAttackIntent::Skill(skill) => skill.to_protocol(),
        }
    }
}

impl<T, U> ToProtocol<ZeroOpt<U>> for Option<T>
where
    T: ToProtocol<U>,
{
    fn to_protocol(self) -> ZeroOpt<U> {
        self.map(|value| value.to_protocol()).into()
    }
}

impl ToProtocolPlayer for PlayerSummary {
    fn to_protocol_player(&self, endpoint: WireServerAddr) -> Player {
        Player {
            db_id: u32::try_from(self.id.get()).unwrap_or(0),
            name: self.name.as_str().into(),
            class_gendered: (self.class, self.gender).to_protocol(),
            level: u8::try_from(self.level).unwrap_or(u8::MAX),
            playtime_minutes: self.playtime.whole_minutes_u32(),
            stat_str: u8::try_from(self.stats.stat_str).unwrap_or(0),
            stat_vit: u8::try_from(self.stats.stat_vit).unwrap_or(0),
            stat_dex: u8::try_from(self.stats.stat_dex).unwrap_or(0),
            stat_int: u8::try_from(self.stats.stat_int).unwrap_or(0),
            body_part: self.appearance.to_protocol() as u16,
            changed_name: 0,
            hair_part: 0,
            server_addr: endpoint,
        }
    }
}

impl ToProtocol<(EntityType, u16)> for EntityKind {
    fn to_protocol(self) -> (EntityType, u16) {
        match self {
            EntityKind::Player { class, gender } => {
                let race: u8 = (class, gender).to_protocol().into();
                (EntityType::Player, race as u16)
            }
            EntityKind::Mob { mob_id, mob_kind } => {
                let entity_type = match mob_kind {
                    MobKind::Npc => EntityType::Npc,
                    MobKind::Monster => EntityType::Monster,
                    MobKind::Stone => EntityType::Stone,
                    MobKind::Portal(zohar_domain::entity::mob::PortalBehavior::MapTransfer) => {
                        EntityType::Warp
                    }
                    MobKind::Portal(zohar_domain::entity::mob::PortalBehavior::LocalReposition) => {
                        EntityType::Goto
                    }
                };
                (entity_type, mob_id.get() as u16)
            }
        }
    }
}

impl ToProtocol<Option<WireStatPoint>> for DomainStat {
    fn to_protocol(self) -> Option<WireStatPoint> {
        Some(match self {
            DomainStat::ArmorDefence
            | DomainStat::MaxHpPrePctBonus
            | DomainStat::BonusMaxHp
            | DomainStat::BonusMaxSp
            | DomainStat::BonusMaxStamina => return None,
            DomainStat::Level => WireStatPoint::Level,
            DomainStat::Exp => WireStatPoint::Exp,
            DomainStat::NextExp => WireStatPoint::NextExp,
            DomainStat::Hp => WireStatPoint::Hp,
            DomainStat::MaxHp => WireStatPoint::MaxHp,
            DomainStat::Sp => WireStatPoint::Sp,
            DomainStat::MaxSp => WireStatPoint::MaxSp,
            DomainStat::Stamina => WireStatPoint::Stamina,
            DomainStat::MaxStamina => WireStatPoint::MaxStamina,
            DomainStat::Gold => WireStatPoint::Gold,
            DomainStat::St => WireStatPoint::St,
            DomainStat::Ht => WireStatPoint::Ht,
            DomainStat::Dx => WireStatPoint::Dx,
            DomainStat::Iq => WireStatPoint::Iq,
            DomainStat::DefGrade => WireStatPoint::DefGrade,
            DomainStat::AttSpeed => WireStatPoint::AttSpeed,
            DomainStat::AttGrade => WireStatPoint::AttGrade,
            DomainStat::MovSpeed => WireStatPoint::MovSpeed,
            DomainStat::DisplayedDefGrade => WireStatPoint::ClientDefGrade,
            DomainStat::CastingSpeed => WireStatPoint::CastingSpeed,
            DomainStat::MagicAttGrade => WireStatPoint::MagicAttGrade,
            DomainStat::MagicDefGrade => WireStatPoint::MagicDefGrade,
            DomainStat::LevelStep => WireStatPoint::LevelStep,
            DomainStat::StatPoints => WireStatPoint::StatPoints,
            DomainStat::HpRecovery => WireStatPoint::HpRecovery,
            DomainStat::SpRecovery => WireStatPoint::SpRecovery,
            DomainStat::ImmuneStun => WireStatPoint::ImmuneStun,
            DomainStat::ImmuneSlow => WireStatPoint::ImmuneSlow,
            DomainStat::ImmuneFall => WireStatPoint::ImmuneFall,
            DomainStat::PartyTankerBonus => WireStatPoint::PartyTankerBonus,
            DomainStat::AttGradeBonus => WireStatPoint::AttGradeBonus,
            DomainStat::DefGradeBonus => WireStatPoint::DefGradeBonus,
            DomainStat::MagicAttGradeBonus => WireStatPoint::MagicAttGradeBonus,
            DomainStat::MagicDefGradeBonus => WireStatPoint::MagicDefGradeBonus,
            DomainStat::PartySkillMasterBonus => WireStatPoint::PartySkillMasterBonus,
            DomainStat::Polymorph => WireStatPoint::Polymorph,
            DomainStat::Mount => WireStatPoint::Mount,
            DomainStat::PartyHasteBonus => WireStatPoint::PartyHasteBonus,
            DomainStat::PartyDefenderBonus => WireStatPoint::PartyDefenderBonus,
            DomainStat::StatResetCount => WireStatPoint::StatResetCount,
            DomainStat::MaxHpPct => WireStatPoint::MaxHpPct,
            DomainStat::MaxSpPct => WireStatPoint::MaxSpPct,
        })
    }
}

impl ToProtocol<WireStatSnapshot> for &StatSnapshot {
    fn to_protocol(self) -> WireStatSnapshot {
        let mut wire = WireStatSnapshot::default();
        for (stat, value) in self.iter() {
            if let Some(point) = stat.to_protocol() {
                wire.set(point, value);
            }
        }
        wire
    }
}
