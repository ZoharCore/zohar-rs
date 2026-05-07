use crate::Empire;
use crate::coords::{Facing72, LocalPos};
use crate::entity::EntityId;
use crate::entity::mob::{MobId, MobKind};
use crate::entity::player::{PlayerClass, PlayerGender};

/// Entity kind with variant-specific data for observer replication.
#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
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

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone)]
pub struct EntityNameplate {
    pub name: String,
    pub empire: Option<Empire>,
    pub level: u32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone)]
pub struct EntitySnapshot {
    pub entity_id: EntityId,
    pub facing: Facing72,
    #[cfg_attr(feature = "admin-brp", reflect(remote = crate::coords::LocalPosReflect))]
    pub pos: LocalPos,
    pub kind: EntityKind,
    pub public_state: EntityPublicState,
    pub nameplate: Option<EntityNameplate>,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntityPublicEquipment {
    pub body_part: u16,
    pub weapon_part: u16,
    pub hair_part: u16,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityPublicSpeeds {
    pub move_speed: u8,
    pub attack_speed: u8,
}

impl Default for EntityPublicSpeeds {
    fn default() -> Self {
        Self {
            move_speed: 100,
            attack_speed: 100,
        }
    }
}

bitflags::bitflags! {
    #[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
    #[cfg_attr(feature = "admin-brp", reflect(opaque))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct EntityStateFlags: u8 {
        const DEAD = 1 << 0;
        const SPAWN = 1 << 1;
    }
}

bitflags::bitflags! {
    #[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
    #[cfg_attr(feature = "admin-brp", reflect(opaque))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct EntityBuffFlags: u64 {
        const SPAWN = 1 << 2;
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntityPublicFlags {
    pub state_flags: EntityStateFlags,
    pub buff_flags: EntityBuffFlags,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntityPublicSocial {
    pub guild_id: u32,
    pub rank_pts: i16,
    pub pvp_mode: u8,
    pub mount_id: u32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntityPublicState {
    pub equipment: EntityPublicEquipment,
    pub speeds: EntityPublicSpeeds,
    pub flags: EntityPublicFlags,
    pub social: EntityPublicSocial,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerVisualProfile {
    pub name: String,
    pub gender: PlayerGender,
    pub empire: Empire,
    pub body_part: u16,
    pub guild_id: u32,
}

impl Default for PlayerVisualProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            gender: PlayerGender::Male,
            empire: Empire::Red,
            body_part: 0,
            guild_id: 0,
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl PlayerAppearance {
    pub fn from_parts(
        visual_profile: &PlayerVisualProfile,
        class: PlayerClass,
        level: u32,
        move_speed: u8,
        attack_speed: u8,
    ) -> Self {
        Self {
            name: visual_profile.name.clone(),
            class,
            gender: visual_profile.gender,
            empire: visual_profile.empire,
            body_part: visual_profile.body_part,
            level,
            guild_id: visual_profile.guild_id,
            move_speed,
            attack_speed,
        }
    }
}

impl Default for PlayerAppearance {
    fn default() -> Self {
        Self::from_parts(
            &PlayerVisualProfile::default(),
            PlayerClass::Warrior,
            1,
            100,
            100,
        )
    }
}
