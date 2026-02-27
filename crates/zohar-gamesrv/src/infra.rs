mod channel_directory;
mod cluster_events;
mod cluster_events_nats;
mod cluster_events_pg;
mod map_resolver;
mod message_bus;

use std::sync::Arc;

pub use channel_directory::{
    ChannelDirectory, ChannelEntry, KubeServiceChannelDirectory, StaticChannelDirectory,
};
pub use cluster_events::{ClusterEvent, ClusterEventBus, GlobalShoutEvent};
pub use cluster_events_nats::NatsClusterEventBusConfig;
pub use map_resolver::{
    EndpointMode, KubeAgonesMapResolver, MapEndpointResolver, MapResolverConfig, StaticMapResolver,
};

/// In-process typed bus for tests and local-only harnesses.
pub fn in_process_cluster_event_bus() -> Arc<ClusterEventBus> {
    Arc::new(cluster_events::ClusterEventBus::in_process())
}

pub fn postgres_cluster_event_bus(
    pool: sqlx::PgPool,
    database_url: impl Into<String>,
) -> Arc<ClusterEventBus> {
    Arc::new(cluster_events::ClusterEventBus::postgres(
        pool,
        database_url,
    ))
}

pub fn nats_cluster_event_bus(
    config: NatsClusterEventBusConfig,
) -> anyhow::Result<Arc<ClusterEventBus>> {
    cluster_events_nats::build_nats_cluster_event_bus(config)
}
