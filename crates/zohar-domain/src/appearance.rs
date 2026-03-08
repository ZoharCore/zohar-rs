use crate::Empire;
use crate::coords::LocalPos;
use crate::entity::EntityId;
use crate::entity::mob::{MobId, MobKind};
use crate::entity::player::{PlayerClass, PlayerGender};

/// Entity kind with variant-specific data for spawn packets.
#[derive(Debug, Clone)]
pub enum EntityKind {
    Player {
        class: PlayerClass,
        gender: PlayerGender,
    },
    Mob {
        mob_id: MobId,
        mob_kind: MobKind,
    },
}

#[derive(Debug, Clone)]
pub struct ShowEntity {
    pub entity_id: EntityId,
    pub angle: f32,
    pub pos: LocalPos,
    pub kind: EntityKind,
    pub move_speed: u8,
    pub attack_speed: u8,
    pub state_flags: u8,
    pub buff_flags: u64,
}

#[derive(Debug, Clone)]
pub struct EntityDetails {
    pub entity_id: EntityId,
    pub name: String,
    pub body_part: u16,
    pub wep_part: u16,
    pub hair_part: u16,
    pub empire: Option<Empire>,
    pub guild_id: u32,
    pub level: u32,
    pub rank_pts: i16,
    pub pvp_mode: u8,
    pub mount_id: u32,
}

#[derive(Debug, Clone)]
pub struct PlayerAppearance {
    pub name: String,
    pub class: PlayerClass,
    pub gender: PlayerGender,
    pub empire: Empire, // pre-computed from empire enum
    pub body_part: u16, // pre-computed from db appearance
    pub level: u32,
    pub guild_id: u32,
    pub move_speed: u8,
    pub attack_speed: u8,
}

impl Default for PlayerAppearance {
    fn default() -> Self {
        Self {
            name: String::new(),
            class: PlayerClass::Warrior,
            gender: PlayerGender::Male,
            empire: Empire::Red,
            body_part: 0,
            level: 1,
            guild_id: 0,
            move_speed: 100,
            attack_speed: 100,
        }
    }
}
