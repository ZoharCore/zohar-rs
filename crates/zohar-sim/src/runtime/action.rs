mod apply;
mod plan;

use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};
use zohar_map_port::{AttackIntent, ClientTimestamp, Facing72, MovementArg, PacketDuration};

use super::state::{MobBrainState, PlayerMotionState, SimDuration};

#[derive(Resource, Default)]
pub(crate) struct ActionBuffer(pub(crate) Vec<Action>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum MobActionCompletion {
    #[default]
    None,
    RethinkAtActionEnd,
    RethinkAtActionEndOrDelay {
        max_delay_ms: SimDuration,
    },
    IdleWander {
        post_move_pause_ms: SimDuration,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    PlayerMotion {
        player_entity: Entity,
        player_id: PlayerId,
        entity_id: EntityId,
        kind: MovementKind,
        arg: MovementArg,
        rot: Facing72,
        end_pos: LocalPos,
        ts: ClientTimestamp,
        duration: PacketDuration,
        motion: PlayerMotionState,
    },
    PlayerAttack {
        player_entity: Entity,
        entity_id: EntityId,
        pos: LocalPos,
        rot: Facing72,
        attack: AttackIntent,
        ts: ClientTimestamp,
        duration: PacketDuration,
    },
    MobMotion {
        mob_entity: Entity,
        entity_id: EntityId,
        start_pos: LocalPos,
        end_pos: LocalPos,
        rot: Facing72,
        kind: MovementKind,
        ts: ClientTimestamp,
        duration: PacketDuration,
        next_brain: MobBrainState,
    },
    MobAttack {
        mob_entity: Entity,
        entity_id: EntityId,
        pos: LocalPos,
        rot: Facing72,
        ts: ClientTimestamp,
        duration: PacketDuration,
        next_brain: MobBrainState,
    },
}

pub(crate) use apply::{apply_action, process_actions, set_mob_brain};
pub(crate) use plan::{
    build_mob_attack_action, build_mob_move_action, build_player_attack_action,
    build_player_move_action,
};
