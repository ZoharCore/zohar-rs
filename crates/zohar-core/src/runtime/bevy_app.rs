use bevy::app::{AppExit, PluginsState};
use bevy::prelude::*;
use std::time::{Duration, Instant};
use zohar_sim::PlayerCount;

const ACTIVE_LOOP_CADENCE: Duration = Duration::from_millis(10);
const IDLE_LOOP_CADENCE: Duration = Duration::from_millis(100);

pub(crate) fn run_map_app(mut app: App) {
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_millis(60));
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
