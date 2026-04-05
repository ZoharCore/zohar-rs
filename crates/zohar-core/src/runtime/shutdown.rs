use bevy::app::AppExit;
use bevy::prelude::*;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use zohar_gamesrv::ServerDrainController;
use zohar_sim::PlayerCount;

#[derive(Resource, Clone)]
pub(crate) struct ShutdownDrainState {
    controller: ServerDrainController,
    grace_period: Duration,
    started_at: Option<Instant>,
    exit_requested: bool,
}

impl ShutdownDrainState {
    pub(crate) fn new(controller: ServerDrainController, grace_period: Duration) -> Self {
        Self {
            controller,
            grace_period,
            started_at: None,
            exit_requested: false,
        }
    }
}

pub(crate) fn poll_server_drain(
    player_count: Res<PlayerCount>,
    mut shutdown: ResMut<ShutdownDrainState>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if shutdown.exit_requested || !shutdown.controller.is_draining() {
        return;
    }

    if shutdown.started_at.is_none() {
        shutdown.started_at = Some(Instant::now());
    }

    let elapsed = shutdown
        .started_at
        .expect("shutdown start time should be initialized")
        .elapsed();
    let active_connections = shutdown.controller.active_connections();
    let active_players = player_count.0;

    if active_connections == 0 && active_players == 0 {
        info!("Server drain completed; shutting down map runtime");
        shutdown.exit_requested = true;
        app_exit.write(AppExit::Success);
        return;
    }

    if elapsed >= shutdown.grace_period {
        warn!(
            active_connections,
            active_players,
            grace_secs = shutdown.grace_period.as_secs_f32(),
            "Server drain grace period expired; forcing shutdown"
        );
        shutdown.exit_requested = true;
        app_exit.write(AppExit::Success);
    }
}

pub(crate) async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
