use crate::bootstrap::content::load_content;
use crate::bootstrap::infra::wire_infra;
use crate::runtime::bevy_app::run_map_app;
use crate::runtime::event_ingress::spawn_cluster_event_ingress;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use zohar_db::postgres_backend;
use zohar_gamesrv::infra::EndpointMode;
use zohar_protocol::token::TokenSigner;
use zohar_sim::MapEventSender;

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
    let token_signer = Arc::new(TokenSigner::new(
        config.auth_token_secret.clone().into_bytes(),
        Duration::from_secs(config.token_window_secs),
    ));

    let (map_events, inbound_rx) = MapEventSender::channel_pair(16_384);
    let loaded = load_content(&config, runtime)?;
    let _map_key = loaded.map_key;

    let wiring = wire_infra(
        &config,
        runtime,
        game_db,
        token_signer,
        loaded.coords.clone(),
        map_events.clone(),
    )?;

    spawn_cluster_event_ingress(
        runtime,
        wiring.ctx.cluster_events.clone(),
        map_events.clone(),
    );

    runtime.spawn(zohar_gamesrv::serve_on_listener(
        wiring.ctx.clone(),
        wiring.listener,
    ));

    run_map_app(loaded.shared_config, loaded.map_config, inbound_rx);
    Ok(())
}
