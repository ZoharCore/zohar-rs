use crate::bootstrap::content::load_content;
use crate::bootstrap::infra::wire_infra;
use crate::runtime::bevy_app::run_map_app;
use crate::runtime::event_ingress::spawn_cluster_event_ingress;
use crate::runtime::player_persistence::spawn_player_persistence_worker;
use crate::runtime::shutdown::{ShutdownDrainState, poll_server_drain, wait_for_shutdown_signal};
use bevy::prelude::*;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use zohar_db::postgres_backend;
use zohar_gamesrv::infra::EndpointMode;
use zohar_gamesrv::{CoreSelectConfig, SERVER_DRAIN_GRACE_PERIOD, ServerDrainController};
use zohar_protocol::token::TokenSigner;
use zohar_sim::{build_map_app, player_persistence_channel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ClusterEventTransport {
    Postgres,
    Nats,
}

pub struct CoreRuntimeConfig {
    pub map: String,
    pub channel: u32,
    pub listen: String,
    pub game_db_url: String,
    pub auth_token_secret: String,
    pub token_window_secs: u64,
    pub login_token_idle_ttl: Duration,
    pub namespace: String,
    pub content_db: PathBuf,
    pub heartbeat_interval: Duration,
    pub active_session_stale_threshold: Duration,
    pub server_id: Option<String>,
    pub map_endpoint_mode: EndpointMode,
    pub map_advertise_ip: Option<Ipv4Addr>,
    pub cluster_event_transport: ClusterEventTransport,
    pub cluster_event_nats_url: Option<String>,
}

pub fn run_core(
    config: CoreRuntimeConfig,
    runtime: &tokio::runtime::Runtime,
) -> anyhow::Result<()> {
    let game_db =
        runtime.block_on(async { postgres_backend::open_game_db(&config.game_db_url).await })?;
    let (player_persistence, player_persistence_rx) = player_persistence_channel(1024);
    let drain = ServerDrainController::new();
    let token_signer = Arc::new(TokenSigner::new(
        config.auth_token_secret.clone().into_bytes(),
        Duration::from_secs(config.token_window_secs),
    ));

    let loaded = load_content(&config, runtime)?;
    let _map_key = loaded.map_key;
    let (mut app, map_events) = build_map_app(
        loaded.shared_config,
        loaded.map_config,
        player_persistence.clone(),
        16_384,
    );

    let wiring = wire_infra(
        &config,
        runtime,
        game_db,
        token_signer,
        loaded.coords.clone(),
        CoreSelectConfig {
            player_create_base_stats: loaded.player_create_base_stats.clone(),
        },
        map_events.clone(),
        player_persistence,
        drain.clone(),
    )?;

    spawn_player_persistence_worker(runtime, wiring.ctx.db.clone(), player_persistence_rx);

    spawn_cluster_event_ingress(
        runtime,
        wiring.ctx.cluster_events.clone(),
        map_events.clone(),
    );

    let server_task = runtime.spawn(zohar_gamesrv::serve_on_listener(
        wiring.ctx.clone(),
        wiring.listener,
    ));
    let server_abort = server_task.abort_handle();
    runtime.spawn({
        let drain = drain.clone();
        async move {
            wait_for_shutdown_signal().await;
            if drain.begin_draining() {
                info!("Shutdown signal received; draining active connections");
                server_abort.abort();
            }
        }
    });

    app.insert_resource(ShutdownDrainState::new(drain, SERVER_DRAIN_GRACE_PERIOD));
    app.add_systems(Update, poll_server_drain);

    run_map_app(app);
    Ok(())
}
