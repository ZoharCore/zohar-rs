use crate::outbox::PlayerOutbox;
use zohar_domain::appearance::{EntityDetails, ShowEntity};
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};
use zohar_domain::{Empire, MapId};

#[derive(Debug)]
pub enum MapCommand {
    Enter {
        player_id: PlayerId,
        map_id: MapId,
        initial_pos: LocalPos,
        outbox: PlayerOutbox,
    },
    Leave {
        player_id: PlayerId,
    },
    ClientIntent {
        player_id: PlayerId,
        intent: ClientIntent,
    },
}

#[derive(Debug, Clone)]
pub enum ClientIntent {
    /// Movement intent with full packet data for broadcast
    Move {
        entity_id: EntityId,
        kind: MovementKind,
        arg: u8,
        rot: u8,
        x: f32,
        y: f32,
        ts: u32,
    },
    Chat {
        message: Vec<u8>,
    },
    Attack {
        target: EntityId,
        attack_type: u8,
    },
}

#[derive(Debug, Clone)]
pub enum MapEvent {
    ToPlayer {
        player_id: PlayerId,
        event: PlayerEvent,
    },
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    EntitySpawn {
        show: ShowEntity,
        details: Option<EntityDetails>,
    },
    EntityMove {
        entity_id: EntityId,
        /// Movement function type (0=wait, 1=move, 2=attack, etc.)
        kind: MovementKind,
        /// Additional argument (e.g. skill slot)
        arg: u8,
        /// Rotation (0-255)
        rot: u8,
        /// Position X
        x: f32,
        /// Position Y
        y: f32,
        /// Client timestamp
        ts: u32,
        /// Movement duration in ms for server-driven movement segments (WAIT/MOVE).
        duration: u32,
    },
    EntityDespawn {
        entity_id: EntityId,
    },
    Chat {
        kind: u8,
        sender_entity_id: Option<EntityId>,
        empire: Option<Empire>,
        message: Vec<u8>,
    },
}
