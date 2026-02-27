use super::cluster_events::ClusterEventBus;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsClusterEventBusConfig {
    pub server_url: String,
    pub subject_prefix: String,
}

pub(crate) fn build_nats_cluster_event_bus(
    config: NatsClusterEventBusConfig,
) -> anyhow::Result<Arc<ClusterEventBus>> {
    anyhow::bail!(
        "NATS cluster-event transport is not implemented yet (server_url='{}', subject_prefix='{}')",
        config.server_url,
        config.subject_prefix
    );
}
