use bevy::prelude::*;
use crossbeam_channel::Receiver;

use crate::bridge::InboundEvent;

use super::action_pipeline::{ActionBuffer, process_actions};
use super::aggro::{MobAggroDispatchBuffer, route_mob_aggro};
use super::chat::process_chat_intents;
use super::idle_chat::emit_idle_chat;
use super::ingress::drain_inbound;
use super::mob_ai::process_mob_ai;
use super::mob_motion::sample_mob_motion;
use super::outbox::outbox_flush;
use super::player_actions::process_player_actions;
use super::players::{on_player_added, on_player_removed};
use super::replication::{aoi_reconcile, replication_flush};
use super::schedule::{advance_sim_time, has_active_players, sync_fixed_tick_rate};
use super::spawn::{bootstrap_map_runtime, signal_startup_ready, spawn_rules};
use super::state::{
    MapConfig, NetEntityIndex, NetworkBridgeRx, PlayerCount, PlayerIndex, RuntimeState,
    SharedConfig, SimSet,
};

pub struct ContentPlugin {
    shared: SharedConfig,
    map: MapConfig,
}

impl ContentPlugin {
    pub fn new(shared: SharedConfig, map: MapConfig) -> Self {
        Self { shared, map }
    }
}

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.shared.clone());
        app.insert_resource(MapConfig {
            map_key: self.map.map_key,
            empire: self.map.empire,
            local_size: self.map.local_size,
            navigator: self.map.navigator.clone(),
            spawn_rules: self.map.spawn_rules.clone(),
        });
    }
}

pub struct NetworkPlugin {
    inbound_rx: Option<Receiver<InboundEvent>>,
}

impl NetworkPlugin {
    pub fn new(inbound_rx: Receiver<InboundEvent>) -> Self {
        Self {
            inbound_rx: Some(inbound_rx),
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        let inbound_rx = self
            .inbound_rx
            .as_ref()
            .expect("NetworkPlugin can only be added once")
            .clone();
        app.insert_resource(NetworkBridgeRx { inbound_rx });
    }
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RuntimeState>()
            .init_resource::<PlayerIndex>()
            .init_resource::<NetEntityIndex>()
            .init_resource::<PlayerCount>()
            .init_resource::<ActionBuffer>()
            .init_resource::<MobAggroDispatchBuffer>();
    }
}

pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PreUpdate,
            (SimSet::DrainInbound, SimSet::SyncTickRate).chain(),
        )
        .add_systems(PreUpdate, drain_inbound.in_set(SimSet::DrainInbound))
        .add_systems(PreUpdate, sync_fixed_tick_rate.in_set(SimSet::SyncTickRate));

        app.configure_sets(
            FixedUpdate,
            (
                SimSet::Sense,
                SimSet::Think,
                SimSet::Act,
                SimSet::Ambient,
                SimSet::AoiReconcile,
            )
                .chain(),
        )
        .configure_sets(
            FixedPostUpdate,
            (SimSet::ReplicationFlush, SimSet::OutboxFlush).chain(),
        )
        .add_systems(Startup, bootstrap_map_runtime)
        .add_systems(PostStartup, signal_startup_ready)
        .add_systems(FixedFirst, advance_sim_time)
        .add_systems(
            FixedUpdate,
            (
                sample_mob_motion,
                spawn_rules,
                process_player_actions,
                route_mob_aggro,
            )
                .chain()
                .in_set(SimSet::Sense),
        )
        .add_systems(FixedUpdate, process_mob_ai.in_set(SimSet::Think))
        .add_systems(FixedUpdate, process_actions.in_set(SimSet::Act))
        .add_systems(
            FixedUpdate,
            (process_chat_intents, emit_idle_chat)
                .chain()
                .in_set(SimSet::Ambient),
        )
        .add_systems(
            FixedUpdate,
            aoi_reconcile
                .in_set(SimSet::AoiReconcile)
                .run_if(has_active_players),
        )
        .add_systems(
            FixedPostUpdate,
            replication_flush.in_set(SimSet::ReplicationFlush),
        )
        .add_observer(on_player_removed)
        .add_observer(on_player_added);
    }
}

pub struct OutboxPlugin;

impl Plugin for OutboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedPostUpdate, outbox_flush.in_set(SimSet::OutboxFlush));
    }
}
