use zohar_domain::Empire;
use zohar_domain::appearance::{EntityPublicState, EntitySnapshot};
use zohar_domain::coords::{Facing72, LocalPos, WorldPos};
use zohar_domain::entity::player::skill::SkillId;
use zohar_domain::entity::player::{CoreStatKind, PlayerId};
use zohar_domain::entity::{EntityId, MovementAnimation, MovementKind};
use zohar_domain::stat::Stat;

use crate::values::{ChatChannel, ClientTimestamp, MovementArg, PacketDuration};

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackIntent {
    Basic,
    Skill(SkillId),
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackTargetIntent {
    pub target: EntityId,
    pub attack: AttackIntent,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetIntent {
    pub target: EntityId,
}

bitflags::bitflags! {
    #[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
    #[cfg_attr(feature = "admin-brp", reflect(opaque))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DamageInfoFlags: u8 {
        const NORMAL = 1 << 0;
        const POISON = 1 << 1;
        const DODGE = 1 << 2;
        const BLOCK = 1 << 3;
        const PENETRATE = 1 << 4;
        const CRITICAL = 1 << 5;
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileEffectKind {
    Exp,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialEffectType {
    Critical,
    Penetrate,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub struct MoveIntent {
    pub kind: MovementKind,
    pub arg: MovementArg,
    pub facing: Facing72,
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub target: LocalPos,
    pub client_ts: ClientTimestamp,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatIntent {
    pub channel: ChatChannel,
    pub message: Vec<u8>,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreStatAllocationIntent {
    pub stat: CoreStatKind,
    pub delta: i8,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillLevelIntent {
    pub skill: SkillId,
    pub delta: i8,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerProgressionIntent {
    CoreStat(CoreStatAllocationIntent),
    SkillLevel(SkillLevelIntent),
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerRestartIntent {
    Here,
    Town,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub struct MovementEvent {
    pub entity_id: EntityId,
    pub kind: MovementKind,
    pub arg: MovementArg,
    pub facing: Facing72,
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub position: LocalPos,
    pub client_ts: ClientTimestamp,
    pub duration: PacketDuration,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatUpdate {
    pub stat: Stat,
    pub absolute: i32,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq)]
pub enum ClientIntent {
    Move(MoveIntent),
    SetMovementAnimation(MovementAnimation),
    Chat(ChatIntent),
    Attack(AttackTargetIntent),
    Target(TargetIntent),
    Progression(PlayerProgressionIntent),
    Restart(PlayerRestartIntent),
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortalDestination {
    MapTransfer {
        #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::WorldPosReflect))]
        world_pos: WorldPos,
    },
    LocalReposition {
        #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
        local_pos: LocalPos,
    },
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    EntitySpawn {
        snapshot: EntitySnapshot,
    },
    SetEntityStats {
        entity_id: EntityId,
        stats: Vec<StatUpdate>,
    },
    SyncEntityHealthBar {
        entity_id: EntityId,
        hp_pct: u8,
    },
    DamageInfo {
        entity_id: EntityId,
        flags: DamageInfoFlags,
        damage: i32,
    },
    CreateProjectileEffect {
        effect: ProjectileEffectKind,
        start_entity_id: EntityId,
        end_entity_id: EntityId,
    },
    SpecialEffect {
        effect: SpecialEffectType,
        entity_id: EntityId,
    },
    EntityStunned {
        entity_id: EntityId,
    },
    EntityDead {
        entity_id: EntityId,
    },
    EntityPublicStateChanged {
        entity_id: EntityId,
        state: EntityPublicState,
    },
    EntityMove(MovementEvent),
    SetEntityMovementAnimation {
        entity_id: EntityId,
        animation: MovementAnimation,
    },
    EntityDespawn {
        entity_id: EntityId,
    },
    Chat {
        channel: ChatChannel,
        sender_entity_id: Option<EntityId>,
        empire: Option<Empire>,
        message: Vec<u8>,
    },
    RestartTown,
    PortalEntered {
        destination: PortalDestination,
    },
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum MapEvent {
    ToPlayer {
        player_id: PlayerId,
        event: PlayerEvent,
    },
}
