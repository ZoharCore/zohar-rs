mod adapters;
mod app;
mod bootstrap;
mod infra;
mod runtime;

use app::{ClusterEventTransport, CoreRuntimeConfig};
use clap::Parser;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use zohar_gamesrv::infra::EndpointMode;

#[derive(Debug, Parser)]
struct Cli {
    /// Content map code (for example: monkey_dungeon_3)
    #[arg(long)]
    map: String,
    /// Channel id
    #[arg(long)]
    channel: u32,
    /// Local listener bind address.
    #[arg(long, default_value = "0.0.0.0:13000")]
    listen: String,
    /// PostgreSQL connection string.
    #[arg(long, env = "ZOHAR_GAME_DATABASE_URL")]
    game_db_url: String,
    /// Shared auth token secret for token verification.
    #[arg(long, env = "ZOHAR_AUTH_TOKEN_SECRET")]
    auth_token_secret: String,
    /// Token signer window size in seconds.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 30)]
    token_window_secs: u64,
    /// Idle TTL for persisted login tokens in seconds.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 7 * 24 * 60 * 60)]
    login_token_idle_ttl_secs: u64,
    /// Kubernetes namespace (used by resolver integration)
    #[arg(long, default_value = "default")]
    namespace: String,
    /// Content DB path.
    #[arg(long, default_value = "/var/lib/zohar/content.db")]
    content_db: PathBuf,
    /// Connection heartbeat interval in seconds.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 30)]
    heartbeat_interval_secs: u64,
    /// Active session stale threshold in seconds.
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..), default_value_t = 90)]
    session_stale_secs: i64,
    /// Optional server identifier override.
    #[arg(long)]
    server_id: Option<String>,
    /// Log filter used by tracing subscriber.
    #[arg(
        long,
        default_value = "info,zohar_core=info,zohar_gamesrv=info,zohar_db=info"
    )]
    log_filter: String,
    /// Map endpoint resolution mode.
    #[arg(long, default_value = "agones")]
    map_endpoint_mode: EndpointMode,
    /// Optional advertised IPv4 override for service-nodeport endpoint mode.
    #[arg(long, env = "ZOHAR_MAP_ADVERTISE_IP")]
    map_advertise_ip: Option<Ipv4Addr>,
    /// Cluster event transport for cross-process events.
    #[arg(long, default_value = "postgres")]
    cluster_event_transport: ClusterEventTransport,
    /// NATS server URL for cluster events when --cluster-event-transport=nats.
    #[arg(long, env = "ZOHAR_CLUSTER_EVENT_NATS_URL")]
    cluster_event_nats_url: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();

    let global_config_filter = EnvFilter::new(&cli.log_filter);

    let stdout_layer = fmt::layer()
        .with_timer(fmt::time::ChronoLocal::new("%H:%M:%S%.3f".into()))
        .with_thread_ids(true);

    let json_layer = std::env::var("ZOHAR_LOG_JSON_FILE")
        .ok()
        .map(|path| std::fs::File::create(path).map(|file| fmt::layer().json().with_writer(file)))
        .transpose()?;

    tracing_subscriber::registry()
        .with(global_config_filter)
        .with(stdout_layer)
        .with(json_layer)
        .init();

    info!("Starting core server...");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    if cli.map_endpoint_mode == EndpointMode::ServiceNodePort && cli.map_advertise_ip.is_none() {
        return Err(anyhow::anyhow!(
            "map endpoint mode 'service-nodeport' requires --map-advertise-ip / ZOHAR_MAP_ADVERTISE_IP"
        ));
    }
    if cli.cluster_event_transport == ClusterEventTransport::Nats
        && cli.cluster_event_nats_url.is_none()
    {
        return Err(anyhow::anyhow!(
            "cluster event transport 'nats' requires --cluster-event-nats-url / ZOHAR_CLUSTER_EVENT_NATS_URL"
        ));
    }

    let runtime_config = CoreRuntimeConfig {
        map: cli.map,
        channel: cli.channel,
        listen: cli.listen,
        game_db_url: cli.game_db_url,
        auth_token_secret: cli.auth_token_secret,
        token_window_secs: cli.token_window_secs,
        login_token_idle_ttl: Duration::from_secs(cli.login_token_idle_ttl_secs),
        namespace: cli.namespace,
        content_db: cli.content_db,
        heartbeat_interval: Duration::from_secs(cli.heartbeat_interval_secs),
        active_session_stale_threshold: Duration::from_secs(cli.session_stale_secs as u64),
        server_id: cli.server_id,
        map_endpoint_mode: cli.map_endpoint_mode,
        map_advertise_ip: cli.map_advertise_ip,
        cluster_event_transport: cli.cluster_event_transport,
        cluster_event_nats_url: cli.cluster_event_nats_url,
    };

    app::run_core(runtime_config, &runtime)
}
