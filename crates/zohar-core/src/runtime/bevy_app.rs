use bevy::app::{AppExit, PluginsState};
use bevy::prelude::*;
use crossbeam_channel::Receiver;
use std::time::{Duration, Instant};
use zohar_sim::bridge::InboundEvent;
use zohar_sim::{
    ContentPlugin, MapConfig, MapPlugin, NetworkPlugin, OutboxPlugin, PlayerCount, SharedConfig,
    SimulationPlugin,
};

const ACTIVE_LOOP_CADENCE: Duration = Duration::from_millis(10);
const IDLE_LOOP_CADENCE: Duration = Duration::from_millis(100);

pub(crate) fn run_map_app(
    shared_config: SharedConfig,
    map_config: MapConfig,
    inbound_rx: Receiver<InboundEvent>,
) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(Time::<Fixed>::from_hz(25.0));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_millis(60));
    app.add_plugins((
        ContentPlugin::new(shared_config, map_config),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.set_runner(adaptive_runtime_runner);
    app.run();
}

fn adaptive_runtime_runner(mut app: App) -> AppExit {
    if app.plugins_state() != PluginsState::Cleaned {
        while app.plugins_state() == PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
    }

    loop {
        let frame_start = Instant::now();
        app.update();

        if let Some(exit) = app.should_exit() {
            return exit;
        }

        let cadence = runtime_cadence(&app);
        let elapsed = frame_start.elapsed();
        if elapsed < cadence {
            std::thread::sleep(cadence - elapsed);
        }
    }
}

pub(crate) fn runtime_cadence(app: &App) -> Duration {
    match app.world().get_resource::<PlayerCount>() {
        Some(player_count) if player_count.0 > 0 => ACTIVE_LOOP_CADENCE,
        _ => IDLE_LOOP_CADENCE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_is_idle_without_players() {
        let app = App::new();
        assert_eq!(runtime_cadence(&app), Duration::from_millis(100));
    }

    #[test]
    fn cadence_is_active_with_players() {
        let mut app = App::new();
        app.insert_resource(PlayerCount(1));
        assert_eq!(runtime_cadence(&app), Duration::from_millis(10));
    }
}
