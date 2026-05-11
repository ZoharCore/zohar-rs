//! Bevy-backed map simulation runtime and protocol-facing map APIs.

pub mod core;
pub mod net;
pub mod runtime;
pub mod spatial;

pub use crate::core::chat::{MobChatContent, MobChatLine, MobChatStrategyInterval};
pub use crate::core::motion::{
    EntityMotionSpeedTable, MobMotionSpeedTable, MobMotionSpeeds, MotionEntityKey, MotionMoveMode,
    PlayerMotionProfileKey, PlayerMotionSpeedTable, PlayerMotionSpeeds,
};
pub use crate::core::persistence::{
    PlayerPersistenceCoordinatorHandle, PlayerPersistenceQueueError, PlayerPersistenceRequest,
    PlayerPersistenceResult, SaveUrgency, player_persistence_channel,
};
pub use crate::core::types::{InstanceId, MapInstanceKey, MapInstanceKind};
pub use crate::net::bridge::MapEventSender;
pub use crate::spatial::navigation::{GridCell, MapNavigator, NavPath, TerrainFlagsGrid};
pub use runtime::{
    MapConfig, PlayerCount, SharedConfig, SimSet, StartupReadySignal, WanderConfig, build_map_app,
    spawn_map_runtime,
};
pub use zohar_gameplay::stats::game::{
    HydratedPlayerStats, LevelExpEntry, LevelExpTable, PlayerClassStatsConfig,
    PlayerClassStatsTable, PlayerStatRules,
};
