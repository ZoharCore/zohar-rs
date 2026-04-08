//! Phase-specific connection handlers.
//!
//! Each phase has its own handler that takes `Connection<Phase>` and returns
//! `Connection<NextPhase>` on successful transition.

mod control;
mod runtime;
mod session_health;
mod types;

mod handshake;
mod ingame;
mod loading;
mod login;
mod select;

use crate::{GameContext, GatewayContext};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tracing::{debug, info, instrument};
use uuid::Uuid;
use zohar_db::{GameDb, SessionsView};
use zohar_net::connection::game_conn::HandshakeGame as Handshake;
use zohar_net::{Connection, ShortId};
use zohar_protocol::handshake::HandshakeState;

use session_health::SessionTracker;
use types::{SessionEnd, SessionLeaseAction};

pub(super) fn connection_id_string(conn_id: Uuid) -> String {
    conn_id.to_string()
}

/// Handle a game connection through all phases.
///
/// Phase chain: Handshake → Login → Select → Loading → Game
#[instrument(skip_all, fields(conn_id = %ShortId(conn_id)))]
pub async fn handle_conn_core(
    stream: TcpStream,
    server_start: Instant,
    conn_id: Uuid,
    ctx: Arc<GameContext>,
) {
    info!("New connection");
    enable_tcp_nodelay(&stream, "core");
    let _conn_guard = ctx.drain.track_connection();

    if ctx.drain.is_draining() {
        info!("Rejecting connection because the server is draining");
        return;
    }

    // Start in Handshake state
    let conn = Connection::<Handshake>::new(stream);

    let mut handshake = HandshakeState::new(server_start, Instant::now());
    let res = run_core_connection(conn_id, &ctx, &mut handshake, conn).await;

    match res {
        Err(end) => match end {
            SessionEnd::AfterLogin {
                username,
                lease_action,
            } => match lease_action {
                SessionLeaseAction::Release => on_session_end(&ctx, &username, conn_id).await,
                SessionLeaseAction::AlreadyReleased => {
                    debug!(
                        username,
                        conn_id = %ShortId(conn_id),
                        "Session lease was already released transactionally during disconnect finalize"
                    );
                }
                SessionLeaseAction::RetainUntilStale => {
                    debug!(
                        username,
                        conn_id = %ShortId(conn_id),
                        "Retaining active session lease until stale-session recovery"
                    );
                }
            },
            SessionEnd::Handoff { username } => {
                debug!(
                    username,
                    conn_id = %ShortId(conn_id),
                    "Session lease was already released transactionally during player handoff"
                );
            }
            SessionEnd::BeforeLogin => {}
        },
        Ok(never) => match never {},
    }
}

/// Backward-compatible alias for tests that still call `handle_conn`.
#[instrument(skip_all, fields(conn_id = %ShortId(conn_id)))]
pub async fn handle_conn(
    stream: TcpStream,
    server_start: Instant,
    conn_id: Uuid,
    ctx: Arc<GameContext>,
) {
    handle_conn_core(stream, server_start, conn_id, ctx).await;
}

/// Handle a channel gateway connection.
///
/// Phase chain: Handshake → Login → Select(browse-only terminal)
#[instrument(skip_all, fields(conn_id = %ShortId(conn_id)))]
pub async fn handle_conn_gateway(
    stream: TcpStream,
    server_start: Instant,
    conn_id: Uuid,
    ctx: Arc<GatewayContext>,
) {
    info!("New gateway connection");
    enable_tcp_nodelay(&stream, "gateway");

    let conn = Connection::<Handshake>::new(stream);
    let mut handshake = HandshakeState::new(server_start, Instant::now());
    let _ = run_gateway_connection(&ctx, &mut handshake, conn).await;
}

fn enable_tcp_nodelay(stream: &TcpStream, connection_role: &'static str) {
    if let Err(error) = stream.set_nodelay(true) {
        debug!(
            connection_role,
            error = %error,
            "Failed to enable TCP_NODELAY"
        );
    }
}

async fn on_session_end(ctx: &GameContext, username: &str, conn_id: Uuid) {
    if let Err(error) = ctx
        .db
        .sessions()
        .release(username, &ctx.server_id, &connection_id_string(conn_id))
        .await
    {
        debug!(
            username,
            conn_id = %ShortId(conn_id),
            error = ?error,
            "Failed to release active session"
        );
    }
}

async fn run_core_connection(
    conn_id: Uuid,
    ctx: &Arc<GameContext>,
    handshake: &mut HandshakeState,
    conn: Connection<Handshake>,
) -> Result<Infallible, SessionEnd> {
    let conn =
        handshake::run_handshake(conn, handshake, ctx.advertised_endpoint.port(), None).await?;
    let mut session = SessionTracker::new(Instant::now(), ctx.heartbeat_interval);
    let conn = login::run_login_core(conn_id, conn, ctx, handshake, &mut session).await?;

    let mut conn = conn;
    loop {
        let conn_loading = select::run_select_core(conn, ctx, handshake, &mut session).await?;
        let conn_ingame = loading::run_loading(conn_loading, ctx, handshake, &mut session).await?;
        let conn_next =
            ingame::run_ingame(conn_id, conn_ingame, ctx, handshake, &mut session).await?;
        conn = conn_next;
    }
}

async fn run_gateway_connection(
    ctx: &Arc<GatewayContext>,
    handshake: &mut HandshakeState,
    conn: Connection<Handshake>,
) -> Result<Infallible, SessionEnd> {
    let conn = handshake::run_handshake(
        conn,
        handshake,
        ctx.advertised_endpoint.port(),
        Some(Arc::clone(&ctx.channel_directory)),
    )
    .await?;
    let mut session = SessionTracker::new(Instant::now(), ctx.heartbeat_interval);
    let conn = login::run_login_gateway(conn, ctx, handshake, &mut session).await?;
    select::run_select_gateway(conn, ctx, handshake, &mut session).await
}
