use bevy::prelude::*;
use crossbeam_channel::Receiver;
use std::time::Duration;

use crate::bridge::InboundEvent;

use super::idle_chat::emit_idle_chat;
use super::ingress::drain_inbound;
use super::movement::process_intents;
use super::outbox::outbox_flush;
use super::players::{on_player_added, on_player_removed};
use super::replication::{aoi_reconcile, replication_flush};
use super::spawn::{bootstrap_map_runtime, signal_startup_ready, spawn_rules};
use super::state::{
    MapConfig, NetEntityIndex, NetworkBridgeRx, PlayerCount, PlayerIndex, RuntimeState,
    SharedConfig, SimSet,
};
use super::util::packet_time_ms;
use super::wander::monster_wander;

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
            .init_resource::<PlayerCount>();
    }
}

pub struct SimulationPlugin;

const ACTIVE_SIM_TIMESTEP: Duration = Duration::from_millis(40);
const IDLE_SIM_TIMESTEP: Duration = Duration::from_secs(1);

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
                SimSet::ProcessIntents,
                SimSet::SpawnRules,
                SimSet::MonsterWander,
                SimSet::IdleChat,
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
        .add_systems(FixedUpdate, process_intents.in_set(SimSet::ProcessIntents))
        .add_systems(FixedUpdate, spawn_rules.in_set(SimSet::SpawnRules))
        .add_systems(FixedUpdate, monster_wander.in_set(SimSet::MonsterWander))
        .add_systems(FixedUpdate, emit_idle_chat.in_set(SimSet::IdleChat))
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

fn advance_sim_time(mut state: ResMut<RuntimeState>) {
    state.sim_time_ms = u64::from(packet_time_ms(state.packet_time_start));
}

fn sync_fixed_tick_rate(player_count: Res<PlayerCount>, mut fixed_time: ResMut<Time<Fixed>>) {
    let previous = fixed_time.timestep();
    let target = if player_count.0 > 0 {
        ACTIVE_SIM_TIMESTEP
    } else {
        IDLE_SIM_TIMESTEP
    };

    if previous == target {
        return;
    }

    // Avoid a large one-frame catch-up burst when a player joins while idle.
    if previous == IDLE_SIM_TIMESTEP && target == ACTIVE_SIM_TIMESTEP {
        let overstep = fixed_time.overstep();
        fixed_time.discard_overstep(overstep);
    }
    fixed_time.set_timestep(target);
}

fn has_active_players(player_count: Res<PlayerCount>) -> bool {
    player_count.0 > 0
}
