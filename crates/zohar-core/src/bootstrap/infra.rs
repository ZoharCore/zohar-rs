use crate::app::{ClusterEventTransport, CoreRuntimeConfig};
use crate::infra::endpoint::resolve_advertised_endpoint;
use anyhow::{Context, anyhow};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::warn;
use zohar_db::Game;
use zohar_gamesrv::infra::{
    KubeAgonesMapResolver, MapEndpointResolver, MapResolverConfig, NatsClusterEventBusConfig,
    nats_cluster_event_bus, postgres_cluster_event_bus,
};
use zohar_gamesrv::{ContentCoords, GameContext};
use zohar_protocol::token::TokenSigner;
use zohar_sim::MapEventSender;

pub(crate) struct InfraWiring {
    pub(crate) listener: TcpListener,
    pub(crate) ctx: Arc<GameContext>,
}

pub(crate) fn wire_infra(
    config: &CoreRuntimeConfig,
    runtime: &tokio::runtime::Runtime,
    game_db: Game,
    token_signer: Arc<TokenSigner>,
    coords: Arc<ContentCoords>,
    map_events: MapEventSender,
) -> anyhow::Result<InfraWiring> {
    let listener = runtime.block_on(async { TcpListener::bind(&config.listen).await })?;
    let local_addr = listener.local_addr()?;
    let advertised_endpoint = runtime
        .block_on(resolve_advertised_endpoint(local_addr))
        .context("resolve advertised endpoint")?;

    let kube_client = runtime.block_on(async { kube::Client::try_default().await.ok() });
    if kube_client.is_none() {
        warn!("Kubernetes client unavailable; map resolver will not be able to resolve maps");
    }
    let resolver_impl = KubeAgonesMapResolver::new(
        kube_client,
        config.namespace.clone(),
        MapResolverConfig::new(config.map_endpoint_mode, config.map_advertise_ip),
    );
    let map_resolver: Arc<MapEndpointResolver> = Arc::new(resolver_impl.into());

    let cluster_events = match config.cluster_event_transport {
        ClusterEventTransport::Postgres => {
            postgres_cluster_event_bus(game_db.pool().clone(), config.game_db_url.clone())
        }
        ClusterEventTransport::Nats => nats_cluster_event_bus(NatsClusterEventBusConfig {
            server_url: config
                .cluster_event_nats_url
                .clone()
                .ok_or_else(|| anyhow!("missing NATS URL for cluster event transport"))?,
            subject_prefix: "zohar.cluster".to_string(),
        })?,
    };

    let server_id = config
        .server_id
        .clone()
        .unwrap_or_else(|| format!("core-ch{}-{}", config.channel, config.map));

    let ctx = Arc::new(GameContext {
        db: game_db,
        token_signer,
        login_token_idle_ttl: config.login_token_idle_ttl,
        coords,
        heartbeat_interval: config.heartbeat_interval,
        server_id,
        active_session_stale_threshold: config.active_session_stale_threshold,
        channel_id: config.channel,
        map_events,
        advertised_endpoint,
        map_code: config.map.clone(),
        map_resolver,
        cluster_events,
    });

    Ok(InfraWiring { listener, ctx })
}
