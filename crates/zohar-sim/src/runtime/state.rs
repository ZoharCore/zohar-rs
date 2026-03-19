use crate::aoi::SpatialIndex;
use crate::bridge::InboundEvent;
use crate::chat::MobChatContent;
use crate::motion::EntityMotionSpeedTable;
use crate::navigation::MapNavigator;
use crate::outbox::PlayerOutbox;
use crate::replication::ReplicationGraph;
use crate::types::MapInstanceKey;
use bevy::prelude::*;
use crossbeam_channel::Receiver;
use rand::rngs::SmallRng;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use zohar_domain::Empire;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::{LocalPos, LocalSize};
use zohar_domain::entity::mob::MobId;
use zohar_domain::entity::mob::MobPrototype;
use zohar_domain::entity::mob::spawn::SpawnRule;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};

pub(super) const DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC: f32 = 4.5;
pub(super) const MAX_MOVE_PACKET_STEP_M: f32 = 25.0;
pub(super) const MAX_MOVE_INTENTS_PER_TICK: usize = 64;
pub(super) const MAX_CHAT_INTENTS_PER_TICK: usize = 16;
pub(super) const MAX_ATTACK_INTENTS_PER_TICK: usize = 16;

#[derive(Debug, Clone)]
pub struct WanderConfig {
    pub decision_pause_idle_min: Duration,
    pub decision_pause_idle_max: Duration,
    pub post_move_pause_min: Duration,
    pub post_move_pause_max: Duration,
    pub wander_chance_denominator: u32,
    pub step_min_m: f32,
    pub step_max_m: f32,
}

impl Default for WanderConfig {
    fn default() -> Self {
        Self {
            decision_pause_idle_min: Duration::from_secs(3),
            decision_pause_idle_max: Duration::from_secs(5),
            post_move_pause_min: Duration::from_secs(1),
            post_move_pause_max: Duration::from_secs(3),
            wander_chance_denominator: 7,
            step_min_m: 3.0,
            step_max_m: 7.0,
        }
    }
}

#[derive(Resource, Clone)]
pub struct SharedConfig {
    pub motion_speeds: Arc<EntityMotionSpeedTable>,
    pub mobs: Arc<HashMap<MobId, MobPrototype>>,
    pub wander: WanderConfig,
    pub mob_chat: Arc<MobChatContent>,
}

#[derive(Resource)]
pub struct MapConfig {
    pub map_key: MapInstanceKey,
    pub empire: Option<Empire>,
    pub local_size: LocalSize,
    pub navigator: Option<Arc<MapNavigator>>,
    pub spawn_rules: Vec<SpawnRule>,
}

#[derive(Resource, Default)]
pub struct PlayerCount(pub u32);

#[derive(Resource)]
pub(crate) struct NetworkBridgeRx {
    pub(crate) inbound_rx: Receiver<InboundEvent>,
}

#[derive(Resource, Default)]
pub struct StartupReadySignal(pub(super) Mutex<Option<oneshot::Sender<()>>>);

impl StartupReadySignal {
    pub fn new(tx: oneshot::Sender<()>) -> Self {
        Self(Mutex::new(Some(tx)))
    }
}

#[derive(Resource)]
pub(super) struct RuntimeState {
    pub(super) next_net_id: u32,
    pub(super) next_pack_id: u32,
    pub(super) map_entity: Option<Entity>,
    pub(super) is_dirty: bool,
    pub(super) sim_time_ms: u64,
    pub(super) packet_time_start: Instant,
    pub(super) rng: SmallRng,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            next_net_id: 0,
            next_pack_id: 0,
            map_entity: None,
            is_dirty: false,
            sim_time_ms: 0,
            packet_time_start: Instant::now(),
            rng: rand::make_rng(),
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct PlayerIndex(pub(super) HashMap<PlayerId, Entity>);

#[derive(Resource, Default)]
pub(super) struct NetEntityIndex(pub(super) HashMap<EntityId, Entity>);

#[derive(Debug, Clone)]
pub(super) struct SpawnRuleState {
    pub(super) rule: SpawnRule,
    pub(super) active_instances: usize,
    pub(super) entities: HashSet<EntityId>,
    pub(super) respawn_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingMovement {
    pub(super) mover_player_id: Option<PlayerId>,
    pub(super) entity_id: EntityId,
    pub(super) new_pos: LocalPos,
    pub(super) kind: MovementKind,
    pub(super) reliable: bool,
    pub(super) arg: u8,
    pub(super) rot: u8,
    pub(super) ts: u32,
    pub(super) duration: u32,
}

#[derive(Debug, Clone)]
pub(super) struct PendingLocalChat {
    pub(super) speaker_player_id: PlayerId,
    pub(super) speaker_entity_id: EntityId,
    pub(super) speaker_empire: Empire,
    pub(super) speaker_name: String,
    pub(super) message: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerMotionState {
    pub(super) segment_start_pos: LocalPos,
    pub(super) segment_end_pos: LocalPos,
    pub(super) segment_start_ts: u32,
    pub(super) segment_end_ts: u32,
    pub(super) last_client_ts: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MobMotionState {
    pub(super) segment_start_pos: LocalPos,
    pub(super) segment_end_pos: LocalPos,
    pub(super) segment_start_at_ms: u64,
    pub(super) segment_end_at_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MoveIntent {
    pub(super) kind: MovementKind,
    pub(super) arg: u8,
    pub(super) rot: u8,
    pub(super) target: LocalPos,
    pub(super) ts: u32,
}

#[derive(Debug, Clone)]
pub(super) struct ChatIntent {
    pub(super) message: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AttackIntent {
    pub(super) target: EntityId,
    pub(super) attack_type: u8,
}

#[derive(Component)]
pub(super) struct MapMarker;

#[derive(Component)]
pub(super) struct MapEmpire(pub(super) Option<Empire>);

#[derive(Component)]
pub(super) struct MapSpatial(pub(super) SpatialIndex);

#[derive(Component, Default)]
pub(super) struct MapReplication(pub(super) ReplicationGraph);

#[derive(Component)]
pub(super) struct MapSpawnRules {
    pub(super) rules: Vec<SpawnRuleState>,
    pub(super) scheduled_spawns: BinaryHeap<Reverse<(u64, usize)>>,
}

#[derive(Component, Default)]
pub(super) struct MapPendingMovements(pub(super) Vec<PendingMovement>);

#[derive(Component, Default)]
pub(super) struct MapPendingLocalChats(pub(super) Vec<PendingLocalChat>);

#[derive(Component)]
pub(super) struct PlayerMarker {
    pub(super) player_id: PlayerId,
}

#[derive(Component)]
pub(super) struct NetEntityId {
    pub(super) net_id: EntityId,
}

#[derive(Component)]
pub(super) struct LocalTransform {
    pub(super) pos: LocalPos,
    pub(super) rot: u8,
}

#[derive(Component)]
pub(super) struct PlayerMotion(pub(super) PlayerMotionState);

#[derive(Component)]
pub(super) struct MobMotion(pub(super) MobMotionState);

#[derive(Component)]
pub(super) struct PlayerAppearanceComp(pub(super) PlayerAppearance);

#[derive(Component)]
pub(super) struct PlayerOutboxComp(pub(super) PlayerOutbox);

#[derive(Component, Default)]
pub(super) struct MoveIntentQueue(pub(super) Vec<MoveIntent>);

#[derive(Component, Default)]
pub(super) struct ChatIntentQueue(pub(super) Vec<ChatIntent>);

#[derive(Component, Default)]
pub(super) struct AttackIntentQueue(pub(super) Vec<AttackIntent>);

#[derive(Component)]
pub(super) struct MobMarker;

#[derive(Component)]
pub(super) struct MobRef {
    pub(super) mob_id: MobId,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct MobPackId {
    pub(super) pack_id: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MobHomeAnchor {
    pub(super) pos: LocalPos,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MobBrainMode {
    Idle,
    Chasing,
    Returning,
    Attacking,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MobBrainState {
    pub(super) mode: MobBrainMode,
    pub(super) target: Option<EntityId>,
    pub(super) target_locked_at_ms: u64,
    pub(super) next_attack_at_ms: u64,
    pub(super) attack_windup_until_ms: u64,
    pub(super) next_chase_rethink_at_ms: u64,
    pub(super) next_wander_decision_at_ms: u64,
    pub(super) wander_wait_until_ms: Option<u64>,
}

impl Default for MobBrainState {
    fn default() -> Self {
        Self {
            mode: MobBrainMode::Idle,
            target: None,
            target_locked_at_ms: 0,
            next_attack_at_ms: 0,
            attack_windup_until_ms: 0,
            next_chase_rethink_at_ms: 0,
            next_wander_decision_at_ms: 0,
            wander_wait_until_ms: None,
        }
    }
}

#[derive(Component)]
pub(super) struct MobChatState {
    pub(super) next_emit_at_ms: u64,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    DrainInbound,
    SyncTickRate,
    ProcessIntents,
    SampleMobMotion,
    SpawnRules,
    AttackIntents,
    MobBrain,
    MobChase,
    IdleChat,
    AoiReconcile,
    ReplicationFlush,
    OutboxFlush,
}
