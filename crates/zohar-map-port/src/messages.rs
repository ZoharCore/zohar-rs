use zohar_domain::Empire;
use zohar_domain::appearance::{EntityDetails, ShowEntity};
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::player::skill::SkillId;
use zohar_domain::entity::{EntityId, MovementKind};

use crate::values::{ChatChannel, ClientTimestamp, Facing72, MovementArg, PacketDuration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackIntent {
    Basic,
    Skill(SkillId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackTargetIntent {
    pub target: EntityId,
    pub attack: AttackIntent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoveIntent {
    pub kind: MovementKind,
    pub arg: MovementArg,
    pub facing: Facing72,
    pub target: LocalPos,
    pub client_ts: ClientTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatIntent {
    pub channel: ChatChannel,
    pub message: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovementEvent {
    pub entity_id: EntityId,
    pub kind: MovementKind,
    pub arg: MovementArg,
    pub facing: Facing72,
    pub position: LocalPos,
    pub client_ts: ClientTimestamp,
    pub duration: PacketDuration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientIntent {
    Move(MoveIntent),
    Chat(ChatIntent),
    Attack(AttackTargetIntent),
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    EntitySpawn {
        show: ShowEntity,
        details: Option<EntityDetails>,
    },
    EntityMove(MovementEvent),
    EntityDespawn {
        entity_id: EntityId,
    },
    Chat {
        channel: ChatChannel,
        sender_entity_id: Option<EntityId>,
        empire: Option<Empire>,
        message: Vec<u8>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum MapEvent {
    ToPlayer {
        player_id: PlayerId,
        event: PlayerEvent,
    },
}
