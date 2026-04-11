mod adapters;
mod coords;
pub mod drain;
pub mod handlers;
pub mod infra;
mod new_player_defaults;

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use zohar_db::Game;
use zohar_net::{listen, listen_on, listen_with_ready};
use zohar_protocol::token::TokenSigner;
use zohar_sim::{MapEventSender, PlayerPersistenceCoordinatorHandle};

pub use coords::{ContentCoords, PersistedPlayerPos, ResolvedSpawn};
pub use drain::ServerDrainController;
pub use infra::{ChannelDirectory, ClusterEventBus, MapEndpointResolver};
pub use new_player_defaults::{EmpireStartMaps, PlayerCreateBaseStatTable, PlayerCreateBaseStats};

pub const SERVER_DRAIN_GRACE_PERIOD: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct CoreSelectConfig {
    pub player_create_base_stats: Arc<PlayerCreateBaseStatTable>,
}

#[derive(Clone)]
pub struct GatewaySelectConfig {
    pub player_create_base_stats: Arc<PlayerCreateBaseStatTable>,
    pub empire_start_maps: EmpireStartMaps,
}

/// Shared context for all game server connections.
#[derive(Clone)]
pub struct GameContext {
    pub db: Game,
    pub select: CoreSelectConfig,
    pub token_signer: Arc<TokenSigner>,
    pub login_token_idle_ttl: Duration,
    pub coords: Arc<ContentCoords>,
    pub heartbeat_interval: Duration,
    pub server_id: String,
    pub active_session_stale_threshold: Duration,
    pub channel_id: u32,
    pub map_events: MapEventSender,
    pub advertised_endpoint: SocketAddr,
    pub map_code: String,
    pub map_resolver: Arc<MapEndpointResolver>,
    pub cluster_events: Arc<ClusterEventBus>,
    pub player_persistence: PlayerPersistenceCoordinatorHandle,
    pub drain: ServerDrainController,
}

/// Shared context for all channel-gateway connections.
#[derive(Clone)]
pub struct GatewayContext {
    pub db: Game,
    pub select: GatewaySelectConfig,
    pub token_signer: Arc<TokenSigner>,
    pub login_token_idle_ttl: Duration,
    pub heartbeat_interval: Duration,
    pub channel_id: u32,
    pub advertised_endpoint: SocketAddr,
    pub map_resolver: Arc<MapEndpointResolver>,
    pub channel_directory: Arc<ChannelDirectory>,
}

pub async fn serve(ctx: Arc<GameContext>, addr: String) {
    listen(addr, move |stream, server_start, conn_id| {
        handlers::handle_conn_core(stream, server_start, conn_id, ctx.clone())
    })
    .await;
}

pub async fn serve_on_listener(ctx: Arc<GameContext>, listener: TcpListener) {
    listen_on(listener, move |stream, server_start, conn_id| {
        handlers::handle_conn_core(stream, server_start, conn_id, ctx.clone())
    })
    .await;
}

pub async fn serve_with_ready(
    ctx: Arc<GameContext>,
    addr: String,
    startup_ready_rx: oneshot::Receiver<()>,
    ready_tx: oneshot::Sender<std::io::Result<()>>,
) {
    match tokio::time::timeout(Duration::from_secs(30), startup_ready_rx).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            let _ = ready_tx.send(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "startup readiness signal dropped before map bootstrap completed",
            )));
            return;
        }
        Err(_) => {
            let _ = ready_tx.send(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "startup readiness timed out waiting for map bootstrap",
            )));
            return;
        }
    }

    listen_with_ready(
        addr,
        Some(ready_tx),
        move |stream, server_start, conn_id| {
            handlers::handle_conn_core(stream, server_start, conn_id, ctx.clone())
        },
    )
    .await;
}

pub async fn serve_gateway(ctx: Arc<GatewayContext>, addr: String) {
    listen(addr, move |stream, server_start, conn_id| {
        handlers::handle_conn_gateway(stream, server_start, conn_id, ctx.clone())
    })
    .await;
}

pub async fn serve_gateway_on_listener(ctx: Arc<GatewayContext>, listener: TcpListener) {
    listen_on(listener, move |stream, server_start, conn_id| {
        handlers::handle_conn_gateway(stream, server_start, conn_id, ctx.clone())
    })
    .await;
}
