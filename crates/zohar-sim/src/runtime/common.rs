use crate::aoi::SpatialIndex;
use crate::replication::ReplicationGraph;
use bevy::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use zohar_domain::Empire;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementAnimation, MovementKind};

pub(crate) use crate::runtime::config::{MapConfig, SharedConfig};
pub(crate) use crate::runtime::mob::{
    MobAggro, MobAggroQueue, MobBrainMode, MobBrainState, MobChatState, MobHomeAnchor, MobMarker,
    MobMotion, MobMotionState, MobPackId, MobRef, SpawnRuleState,
};
pub(crate) use crate::runtime::player::{
    ChatIntent, ChatIntentQueue, PlayerAppearanceComp, PlayerCommand, PlayerCommandQueue,
    PlayerMarker, PlayerMotion, PlayerMotionState, PlayerMovementAnimation, PlayerOutboxComp,
};
pub(crate) use crate::runtime::resources::{
    NetEntityIndex, NetworkBridgeRx, PlayerCount, PlayerIndex, RuntimeState, StartupReadySignal,
};
pub(crate) use crate::runtime::schedule::SimSet;
pub(crate) use crate::runtime::time::{SimDuration, SimInstant};
use zohar_map_port::{ChatChannel, ClientTimestamp, Facing72, MovementArg, PacketDuration};

pub(crate) const DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC: f32 = 4.5;
pub(crate) const MAX_MOVE_PACKET_STEP_M: f32 = 25.0;
pub(crate) const MAX_MOVE_INTENTS_PER_TICK: usize = 64;
pub(crate) const MAX_CHAT_INTENTS_PER_TICK: usize = 16;
pub(crate) const MAX_ATTACK_INTENTS_PER_TICK: usize = 16;
pub(crate) const MAX_MOB_STIMULI_PER_TICK: usize = 16;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingMovement {
    pub(crate) mover_player_id: Option<PlayerId>,
    pub(crate) entity_id: EntityId,
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) new_pos: LocalPos,
    pub(crate) kind: MovementKind,
    pub(crate) reliable: bool,
    pub(crate) arg: MovementArg,
    pub(crate) rot: Facing72,
    pub(crate) ts: ClientTimestamp,
    pub(crate) duration: PacketDuration,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone)]
pub(crate) struct PendingLocalChat {
    pub(crate) speaker_player_id: PlayerId,
    pub(crate) speaker_entity_id: EntityId,
    pub(crate) speaker_empire: Empire,
    pub(crate) channel: ChatChannel,
    pub(crate) speaker_name: String,
    pub(crate) message: Vec<u8>,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingMovementAnimation {
    pub(crate) entity_id: EntityId,
    pub(crate) animation: MovementAnimation,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MapMarker;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MapEmpire(pub(crate) Option<Empire>);

#[derive(Component)]
pub(crate) struct MapSpatial(pub(crate) SpatialIndex);

#[derive(Component, Default)]
pub(crate) struct MapReplication(pub(crate) ReplicationGraph);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct MapSpawnRules {
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub(crate) rules: Vec<SpawnRuleState>,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub(crate) scheduled_spawns: BinaryHeap<Reverse<(SimInstant, usize)>>,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct MapPendingMovements(pub(crate) Vec<PendingMovement>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct MapPendingLocalChats(pub(crate) Vec<PendingLocalChat>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Default)]
pub(crate) struct MapPendingMovementAnimations(pub(crate) Vec<PendingMovementAnimation>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct NetEntityId {
    pub(crate) net_id: EntityId,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component)]
pub(crate) struct LocalTransform {
    #[cfg_attr(feature = "admin-brp", reflect(remote = zohar_domain::coords::LocalPosReflect))]
    pub(crate) pos: LocalPos,
    pub(crate) rot: Facing72,
}
