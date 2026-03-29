pub(crate) mod action;
pub(crate) mod common;
pub(crate) mod config;
pub(crate) mod mob;
pub(crate) mod net;
pub(crate) mod player;
mod plugins;
pub(crate) mod resources;
pub(crate) mod rules;
pub(crate) mod schedule;
pub(crate) mod spatial;

pub(crate) use action as action_pipeline;
pub(crate) use common as state;
pub(crate) use mob::aggro;
pub(crate) use mob::ai as mob_ai;
pub(crate) use mob::ambient_chat as idle_chat;
pub(crate) use mob::spawn;
pub(crate) use net::ingress;
pub(crate) use net::outbox;
pub(crate) use net::replication;
pub(crate) use player::actions as player_actions;
pub(crate) use player::chat;
pub(crate) use player::lifecycle as players;
pub(crate) use spatial as mob_motion;
pub(crate) use spatial as query;
pub(crate) use spatial as util;

pub use config::{MapConfig, SharedConfig, WanderConfig};
pub use plugins::{ContentPlugin, MapPlugin, NetworkPlugin, OutboxPlugin, SimulationPlugin};
pub use resources::{PlayerCount, StartupReadySignal};
pub use schedule::SimSet;

#[cfg(test)]
mod tests;
