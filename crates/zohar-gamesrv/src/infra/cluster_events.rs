use super::cluster_events_pg::PgClusterEventBus;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use zohar_domain::Empire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterEvent {
    GlobalShout(GlobalShoutEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShoutEvent {
    pub from_player_name: String,
    pub from_empire: Empire,
    pub message: String,
}

#[derive(Clone)]
pub struct ClusterEventBus {
    inner: ClusterEventBusImpl,
}

#[derive(Clone)]
enum ClusterEventBusImpl {
    InProcess(InProcessClusterEventBus),
    Postgres(PgClusterEventBus),
}

impl ClusterEventBus {
    pub(crate) fn in_process() -> Self {
        Self {
            inner: ClusterEventBusImpl::InProcess(InProcessClusterEventBus::default()),
        }
    }

    pub(crate) fn postgres(pool: sqlx::PgPool, database_url: impl Into<String>) -> Self {
        Self {
            inner: ClusterEventBusImpl::Postgres(PgClusterEventBus::new(pool, database_url)),
        }
    }

    pub async fn publish(&self, event: Arc<ClusterEvent>) -> Result<()> {
        match &self.inner {
            ClusterEventBusImpl::InProcess(bus) => bus.publish(event).await,
            ClusterEventBusImpl::Postgres(bus) => bus.publish(event).await,
        }
    }

    pub async fn subscribe(&self) -> Result<broadcast::Receiver<Arc<ClusterEvent>>> {
        match &self.inner {
            ClusterEventBusImpl::InProcess(bus) => bus.subscribe().await,
            ClusterEventBusImpl::Postgres(bus) => bus.subscribe().await,
        }
    }
}

#[derive(Clone)]
pub(crate) struct InProcessClusterEventBus {
    tx: broadcast::Sender<Arc<ClusterEvent>>,
}

impl Default for InProcessClusterEventBus {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }
}

impl InProcessClusterEventBus {
    pub(crate) async fn publish(&self, event: Arc<ClusterEvent>) -> Result<()> {
        let _ = self.tx.send(event);
        Ok(())
    }

    pub(crate) async fn subscribe(&self) -> Result<broadcast::Receiver<Arc<ClusterEvent>>> {
        Ok(self.tx.subscribe())
    }
}
