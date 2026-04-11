use zohar_domain::Empire;
use zohar_domain::appearance::{EntityDetails, ShowEntity};
use zohar_domain::coords::{LocalPos, WorldPos};
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::player::skill::SkillId;
use zohar_domain::entity::{EntityId, MovementAnimation, MovementKind};

use crate::values::{ChatChannel, ClientTimestamp, Facing72, MovementArg, PacketDuration};

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
#[derive(Debug, Clone, PartialEq)]
pub enum ClientIntent {
    Move(MoveIntent),
    SetMovementAnimation(MovementAnimation),
    Chat(ChatIntent),
    Attack(AttackTargetIntent),
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
        show: ShowEntity,
        details: Option<EntityDetails>,
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
