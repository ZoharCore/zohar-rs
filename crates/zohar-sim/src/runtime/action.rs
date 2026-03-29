mod apply;
mod plan;

use bevy::prelude::*;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};

use super::state::{MobBrainState, PlayerMotionState};

#[derive(Resource, Default)]
pub(crate) struct ActionBuffer(pub(crate) Vec<Action>);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum MobActionCompletion {
    #[default]
    None,
    RethinkAtActionEnd,
    RethinkAtActionEndOrDelay {
        max_delay_ms: u64,
    },
    IdleWander {
        post_move_pause_ms: u64,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    PlayerMotion {
        player_entity: Entity,
        player_id: PlayerId,
        entity_id: EntityId,
        kind: MovementKind,
        arg: u8,
        rot: u8,
        end_pos: LocalPos,
        ts: u32,
        duration: u32,
        motion: PlayerMotionState,
    },
    PlayerAttack {
        player_entity: Entity,
        entity_id: EntityId,
        pos: LocalPos,
        rot: u8,
        attack_type: u8,
        ts: u32,
        duration: u32,
    },
    MobMotion {
        mob_entity: Entity,
        entity_id: EntityId,
        start_pos: LocalPos,
        end_pos: LocalPos,
        rot: u8,
        kind: MovementKind,
        ts: u32,
        duration: u32,
        next_brain: MobBrainState,
    },
    MobAttack {
        mob_entity: Entity,
        entity_id: EntityId,
        pos: LocalPos,
        rot: u8,
        ts: u32,
        duration: u32,
        next_brain: MobBrainState,
    },
}

pub(crate) use apply::{apply_action, process_actions, set_mob_brain};
pub(crate) use plan::{
    build_mob_attack_action, build_mob_move_action, build_player_attack_action,
    build_player_move_action,
};
