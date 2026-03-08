use tracing::warn;
use zohar_db::PlayerRow;
use zohar_domain::Empire as DomainEmpire;
use zohar_domain::appearance::EntityKind;
use zohar_domain::entity::mob::MobKind;
use zohar_domain::entity::player::skill::{
    NinjaSkillBranch, ShamanSkillBranch, SkillBranch as DomainSkillBranch, SuraSkillBranch,
    WarriorSkillBranch,
};
use zohar_domain::entity::player::{
    PlayerBaseAppearance as DomainPlayerAppearance, PlayerClass as DomainPlayerClass,
    PlayerGender as DomainPlayerGender, PlayerSlot, PlayerStats, PlayerSummary,
};
use zohar_domain::entity::{EntityId, MovementKind as DomainMovementKind};
use zohar_protocol::game_pkt::ingame::movement::MovementKind;
use zohar_protocol::game_pkt::ingame::world::EntityType;
use zohar_protocol::game_pkt::select::{Player, PlayerBaseAppearance};
use zohar_protocol::game_pkt::{Empire, NetId, PlayerClassGendered, SkillBranch, ZeroOpt};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayerEndpoint {
    pub(crate) srv_ipv4_addr: i32,
    pub(crate) srv_port: u16,
}

pub(crate) trait ToDomain<T> {
    fn to_domain(self) -> T;
}

pub(crate) trait ToProtocol<T> {
    fn to_protocol(self) -> T;
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

impl ToDomain<PlayerSummary> for &PlayerRow {
    fn to_domain(self) -> PlayerSummary {
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

        PlayerSummary {
            id: self.id,
            slot,
            name: self.name.clone(),
            class: self.class,
            gender: self.gender,
            appearance: self.appearance,
            level: self.level,
            stats: PlayerStats {
                stat_str: self.stat_str,
                stat_vit: self.stat_vit,
                stat_dex: self.stat_dex,
                stat_int: self.stat_int,
            },
        }
    }
}

pub(crate) trait ToProtocolPlayer {
    fn to_protocol_player(&self, endpoint: PlayerEndpoint) -> Player;
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

impl<T, U> ToProtocol<ZeroOpt<U>> for Option<T>
where
    T: ToProtocol<U>,
{
    fn to_protocol(self) -> ZeroOpt<U> {
        self.map(|value| value.to_protocol()).into()
    }
}

impl ToProtocolPlayer for PlayerSummary {
    fn to_protocol_player(&self, endpoint: PlayerEndpoint) -> Player {
        Player {
            db_id: u32::try_from(self.id.get()).unwrap_or(0),
            name: self.name.as_str().into(),
            class_gendered: (self.class, self.gender).to_protocol(),
            level: u8::try_from(self.level).unwrap_or(u8::MAX),
            playtime_minutes: 0,
            stat_str: u8::try_from(self.stats.stat_str).unwrap_or(0),
            stat_vit: u8::try_from(self.stats.stat_vit).unwrap_or(0),
            stat_dex: u8::try_from(self.stats.stat_dex).unwrap_or(0),
            stat_int: u8::try_from(self.stats.stat_int).unwrap_or(0),
            body_part: self.appearance.to_protocol() as u16,
            changed_name: 0,
            hair_part: 0,
            pos_x: 0.into(),
            pos_y: 0.into(),
            srv_ipv4_addr: endpoint.srv_ipv4_addr,
            srv_port: endpoint.srv_port,
            skill_branch: None::<DomainSkillBranch>.to_protocol(),
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
                    MobKind::Portal => EntityType::Portal,
                };
                (entity_type, mob_id.get() as u16)
            }
        }
    }
}
