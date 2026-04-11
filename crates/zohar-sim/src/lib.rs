//! Bevy-backed map simulation runtime and protocol-facing map APIs.

pub mod aoi;
mod bridge;
pub mod chat;
pub mod motion;
pub mod navigation;
mod outbox;
pub mod persistence;
mod replication;
pub mod runtime;
pub mod types;

pub use bridge::MapEventSender;
pub use chat::{MobChatContent, MobChatLine, MobChatStrategyInterval};
pub use motion::{
    EntityMotionSpeedTable, MobMotionSpeedTable, MobMotionSpeeds, MotionEntityKey, MotionMoveMode,
    PlayerMotionProfileKey, PlayerMotionSpeedTable, PlayerMotionSpeeds,
};
pub use navigation::{GridCell, MapNavigator, NavPath, TerrainFlagsGrid};
pub use persistence::{
    PlayerPersistenceCoordinatorHandle, PlayerPersistenceQueueError, PlayerPersistenceRequest,
    PlayerPersistenceResult, SaveUrgency, player_persistence_channel,
};
pub use runtime::{
    MapConfig, PlayerCount, SharedConfig, SimSet, StartupReadySignal, WanderConfig, build_map_app,
    spawn_map_runtime,
};
pub use types::{InstanceId, MapInstanceKey, MapInstanceKind};
pub use zohar_gameplay::stats::game::{
    HydratedPlayerStats, LevelExpEntry, LevelExpTable, PlayerClassStatsConfig,
    PlayerClassStatsTable, PlayerStatRules,
};
