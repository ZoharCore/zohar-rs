use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::warn;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use zohar_db::postgres_backend;
use zohar_gamesrv::infra::{
    EndpointMode, KubeAgonesMapResolver, KubeServiceChannelDirectory, MapEndpointResolver,
    MapResolverConfig,
};
use zohar_gamesrv::{EmpireStartMaps, GatewayContext};
use zohar_protocol::token::TokenSigner;

#[derive(Debug, Parser)]
struct Cli {
    /// Channel id this gateway represents
    #[arg(long)]
    channel: u32,
    /// Local listener bind address
    #[arg(long, default_value = "0.0.0.0:13000")]
    listen: String,
    /// PostgreSQL connection string
    #[arg(long, env = "ZOHAR_GAME_DATABASE_URL")]
    game_db_url: String,
    /// Shared auth token secret for token verification.
    #[arg(long, env = "ZOHAR_AUTH_TOKEN_SECRET")]
    auth_token_secret: String,
    /// Token signer window size in seconds.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 30)]
    token_window_secs: u64,
    /// Kubernetes namespace
    #[arg(long, default_value = "default")]
    namespace: String,
    /// Idle TTL for persisted login tokens in seconds.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 7 * 24 * 60 * 60)]
    login_token_idle_ttl_secs: u64,
    /// Connection heartbeat interval
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 30)]
    heartbeat_interval_secs: u64,
    /// Service selector for channel entry services
    #[arg(long, default_value = "app.kubernetes.io/component=channel-entry")]
    channel_service_selector: String,
    /// Log filter used by tracing subscriber.
    #[arg(long, default_value = "info,zohar_gamesrv=info,zohar_db=info")]
    log_filter: String,
    /// Map endpoint resolution mode.
    #[arg(long, default_value = "agones")]
    map_endpoint_mode: EndpointMode,
    /// Optional advertised IPv4 override for service-nodeport endpoint mode.
    #[arg(long, env = "ZOHAR_MAP_ADVERTISE_IP")]
    map_advertise_ip: Option<Ipv4Addr>,
    /// Optional empire fallback map code override when DB has no player map (Red).
    #[arg(long)]
    start_red_map: Option<String>,
    /// Optional empire fallback map code override when DB has no player map (Yellow).
    #[arg(long)]
    start_yellow_map: Option<String>,
    /// Optional empire fallback map code override when DB has no player map (Blue).
    #[arg(long)]
    start_blue_map: Option<String>,
}

fn install_rustls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(EnvFilter::new(&cli.log_filter))
        .with(fmt::layer().with_timer(fmt::time::ChronoLocal::new("%H:%M:%S%.3f".into())))
        .init();

    install_rustls_provider();

    if cli.map_endpoint_mode == EndpointMode::ServiceNodePort && cli.map_advertise_ip.is_none() {
        return Err(anyhow::anyhow!(
            "map endpoint mode 'service-nodeport' requires --map-advertise-ip / ZOHAR_MAP_ADVERTISE_IP"
        ));
    }

    let game_db = postgres_backend::open_game_db(&cli.game_db_url)
        .await
        .context("open game db")?;
    let token_signer = Arc::new(TokenSigner::new(
        cli.auth_token_secret.as_bytes().to_vec(),
        Duration::from_secs(cli.token_window_secs),
    ));

    let empire_start_maps =
        EmpireStartMaps::from_options(cli.start_red_map, cli.start_yellow_map, cli.start_blue_map)
            .validate()
            .context("validate empire fallback map codes")?;

    let listener = TcpListener::bind(&cli.listen)
        .await
        .with_context(|| format!("bind gateway listener {}", cli.listen))?;
    let advertised_endpoint = listener.local_addr().context("gateway local addr")?;

    let kube_client = kube::Client::try_default().await.ok();
    if kube_client.is_none() {
        warn!("kubernetes client unavailable; gateway discovery may fail");
    }

    let map_resolver: Arc<MapEndpointResolver> = Arc::new(
        KubeAgonesMapResolver::new(
            kube_client.clone(),
            cli.namespace.clone(),
            MapResolverConfig::new(cli.map_endpoint_mode, cli.map_advertise_ip),
        )
        .into(),
    );

    let channel_directory = Arc::new(
        KubeServiceChannelDirectory::new(kube_client, cli.namespace, cli.channel_service_selector)
            .into(),
    );

    let ctx = Arc::new(GatewayContext {
        db: game_db,
        token_signer,
        login_token_idle_ttl: Duration::from_secs(cli.login_token_idle_ttl_secs),
        empire_start_maps,
        heartbeat_interval: Duration::from_secs(cli.heartbeat_interval_secs),
        channel_id: cli.channel,
        advertised_endpoint,
        map_resolver,
        channel_directory,
    });

    zohar_gamesrv::serve_gateway_on_listener(ctx, listener).await;
    Ok(())
}
