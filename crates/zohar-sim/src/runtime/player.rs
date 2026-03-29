pub(crate) mod actions;
pub(crate) mod chat;
pub(crate) mod lifecycle;

use crate::outbox::PlayerOutbox;
use bevy::prelude::*;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};

pub(crate) use self::lifecycle as players;
pub(crate) use crate::runtime::action as action_pipeline;
pub(crate) use crate::runtime::common as state;
pub(crate) use crate::runtime::mob::aggro;
pub(crate) use crate::runtime::spatial as query;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayerMotionState {
    pub(crate) segment_start_pos: LocalPos,
    pub(crate) segment_end_pos: LocalPos,
    pub(crate) segment_start_ts: u32,
    pub(crate) segment_end_ts: u32,
    pub(crate) last_client_ts: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlayerCommand {
    Move {
        kind: MovementKind,
        arg: u8,
        rot: u8,
        target: LocalPos,
        ts: u32,
    },
    Attack {
        target: EntityId,
        attack_type: u8,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ChatIntent {
    pub(crate) message: Vec<u8>,
}

#[derive(Component)]
pub(crate) struct PlayerMarker {
    pub(crate) player_id: PlayerId,
}

#[derive(Component)]
pub(crate) struct PlayerMotion(pub(crate) PlayerMotionState);

#[derive(Component)]
pub(crate) struct PlayerAppearanceComp(pub(crate) PlayerAppearance);

#[derive(Component)]
pub(crate) struct PlayerOutboxComp(pub(crate) PlayerOutbox);

#[derive(Component, Default)]
pub(crate) struct PlayerCommandQueue(pub(crate) Vec<PlayerCommand>);

#[derive(Component, Default)]
pub(crate) struct ChatIntentQueue(pub(crate) Vec<ChatIntent>);
