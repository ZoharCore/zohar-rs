pub(crate) mod actions;
pub(crate) mod chat;
pub(crate) mod lifecycle;
pub(crate) mod persistence;

use crate::outbox::PlayerOutbox;
use bevy::prelude::*;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementAnimation, MovementKind};
use zohar_map_port::{AttackIntent, ChatChannel, ClientTimestamp, Facing72, MovementArg};

pub(crate) use self::lifecycle as players;
pub(crate) use crate::runtime::action as action_pipeline;
pub(crate) use crate::runtime::common as state;
pub(crate) use crate::runtime::mob::aggro;
pub(crate) use crate::runtime::spatial as query;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayerMotionState {
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) segment_start_pos: LocalPos,
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) segment_end_pos: LocalPos,
    pub(crate) segment_start_ts: ClientTimestamp,
    pub(crate) segment_end_ts: ClientTimestamp,
    pub(crate) last_client_ts: ClientTimestamp,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy)]
pub(crate) enum PlayerCommand {
    Move {
        kind: MovementKind,
        arg: MovementArg,
        rot: Facing72,
        #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
        target: LocalPos,
        ts: ClientTimestamp,
    },
    SetMovementAnimation(MovementAnimation),
    Attack {
        target: EntityId,
        attack: AttackIntent,
    },
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone)]
pub(crate) struct ChatIntent {
    pub(crate) channel: ChatChannel,
    pub(crate) message: Vec<u8>,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct PlayerMarker {
    pub(crate) player_id: PlayerId,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct PlayerMotion(pub(crate) PlayerMotionState);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct PlayerAppearanceComp(pub(crate) PlayerAppearance);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct PlayerMovementAnimation(pub(crate) MovementAnimation);

#[derive(Component)]
pub(crate) struct PlayerOutboxComp(pub(crate) PlayerOutbox);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct PlayerCommandQueue(pub(crate) Vec<PlayerCommand>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct ChatIntentQueue(pub(crate) Vec<ChatIntent>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct PlayerPersistenceState {
    pub(crate) dirty: bool,
    pub(crate) next_autosave_at: crate::runtime::time::SimInstant,
}
