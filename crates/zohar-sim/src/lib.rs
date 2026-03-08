//! Bevy-backed map simulation runtime and protocol-facing map APIs.

pub mod aoi;
pub mod api;
pub mod bridge;
pub mod chat;
pub mod motion;
pub mod outbox;
mod replication;
pub mod runtime;
pub mod types;

pub use api::{ClientIntent, MapCommand, MapEvent, PlayerEvent};
pub use bridge::{
    ClientIntentMsg, EnterMsg, InboundEvent, LeaveMsg, LocalMapInbound, MapEventSender,
};
pub use chat::{MobChatContent, MobChatLine, MobChatStrategyInterval};
pub use motion::{
    EntityMotionSpeedTable, MobMotionSpeedTable, MobMotionSpeeds, MotionEntityKey, MotionMoveMode,
    PlayerMotionProfileKey, PlayerMotionSpeedTable, PlayerMotionSpeeds,
};
pub use outbox::{PlayerOutbox, PlayerOutboxStats};
pub use runtime::{
    ContentPlugin, MapConfig, MapPlugin, NetworkPlugin, OutboxPlugin, PlayerCount, SharedConfig,
    SimSet, SimulationPlugin, StartupReadySignal, WanderConfig,
};
pub use types::{InstanceId, MapInstanceKey, MapInstanceKind};
