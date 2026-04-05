//! Handshake phase handler.
//!
//! Handles time synchronization and server list queries.

use super::runtime::{PhaseEffects, base_phase_span, disconnect, run_phase};
use super::types::{PhaseResult, SessionEnd};
use crate::ChannelDirectory;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;
use zohar_net::connection::NextConnection;
use zohar_net::connection::game_conn::HandshakeGame as ThisPhase;
use zohar_net::{Connection, ConnectionPhaseExt, ConnectionState};
use zohar_protocol::game_pkt::ControlC2s;
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::handshake::{
    HandshakeGameC2s as HandshakeC2s, HandshakeGameC2sSpecific, HandshakeGameS2cSpecific,
};
use zohar_protocol::game_pkt::{ServerInfo, ServerStatus};
use zohar_protocol::handshake::{HandshakeOutcome, HandshakeState};
struct HandshakeCtx<'a> {
    handshake: &'a mut HandshakeState,
    srv_port: u16,
    channel_directory: Option<Arc<ChannelDirectory>>,
}

async fn handle_enter(ctx: &mut HandshakeCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let now = Instant::now();
    Ok(PhaseEffects::send_many([
        ControlS2c::SetClientPhase {
            phase: <ThisPhase as ConnectionState>::PHASE_ID,
        }
        .into(),
        ControlS2c::RequestHandshake {
            data: ctx.handshake.initial_sync_data(now),
        }
        .into(),
    ]))
}

async fn handle_packet(
    packet: HandshakeC2s,
    ctx: &mut HandshakeCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let now = Instant::now();
    match packet {
        HandshakeC2s::Control(ControlC2s::HandshakeResponse { data }) => {
            let outcome = ctx.handshake.handle(data, now)?;
            match outcome {
                HandshakeOutcome::CompletedInitial => Ok(PhaseEffects::transition(())),
                HandshakeOutcome::SendHandshakeSync(data) => Ok(PhaseEffects::send(
                    ControlS2c::RequestHandshake { data }.into(),
                )),
                HandshakeOutcome::SendTimeSyncAck => Ok(PhaseEffects::empty()),
            }
        }
        HandshakeC2s::Control(ControlC2s::HeartbeatResponse) => Ok(PhaseEffects::disconnect(
            "heartbeat not allowed during handshake",
        )),
        HandshakeC2s::Control(ControlC2s::RequestTimeSync { .. }) => Ok(PhaseEffects::disconnect(
            "time sync request not allowed during handshake",
        )),
        HandshakeC2s::Specific(HandshakeGameC2sSpecific::FetchChannelList) => {
            let (statuses, is_ok) = if let Some(directory) = ctx.channel_directory.as_ref() {
                match directory.list_channels().await {
                    Ok(entries) => (
                        entries
                            .into_iter()
                            .map(|entry| ServerInfo {
                                srv_port: entry.port,
                                status: if entry.ready {
                                    ServerStatus::OnlineBusy
                                } else {
                                    ServerStatus::Offline
                                },
                            })
                            .collect(),
                        true,
                    ),
                    Err(error) => {
                        warn!(error = ?error, "channel directory lookup failed");
                        (
                            vec![ServerInfo {
                                srv_port: ctx.srv_port,
                                status: ServerStatus::OnlineBusy,
                            }],
                            false,
                        )
                    }
                }
            } else {
                (
                    vec![ServerInfo {
                        srv_port: ctx.srv_port,
                        status: ServerStatus::OnlineBusy,
                    }],
                    true,
                )
            };
            Ok(PhaseEffects::send(
                HandshakeGameS2cSpecific::ChannelListResponse {
                    statuses,
                    is_ok: is_ok.into(),
                }
                .into(),
            ))
        }
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<<ThisPhase as zohar_net::connection::NextState>::Data>> {
    for packet in effects.send {
        conn.send(packet).await?;
    }
    if let Some(reason) = effects.disconnect {
        return Err(disconnect(reason));
    }
    Ok(effects.transition)
}

async fn drive_handshake(
    mut conn: Connection<ThisPhase>,
    ctx: &mut HandshakeCtx<'_>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    // Enter phase
    let effects = handle_enter(ctx).await?;
    if let Some(data) = apply_effects(&mut conn, effects).await? {
        return Ok(conn.into_next_with_phase(data).await?);
    }

    // Main loop (no heartbeat for handshake)
    loop {
        let packet = conn
            .recv()
            .await?
            .ok_or_else(|| disconnect("connection closed"))?;
        let effects = handle_packet(packet, ctx).await?;
        if let Some(data) = apply_effects(&mut conn, effects).await? {
            return Ok(conn.into_next_with_phase(data).await?);
        }
    }
}

pub(crate) async fn run_handshake(
    conn: Connection<ThisPhase>,
    handshake: &mut HandshakeState,
    advertised_port: u16,
    channel_directory: Option<Arc<ChannelDirectory>>,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let mut ctx = HandshakeCtx {
        handshake,
        srv_port: advertised_port,
        channel_directory,
    };
    let span = base_phase_span::<ThisPhase>();
    run_phase(
        "Disconnected during handshake",
        SessionEnd::BeforeLogin,
        span,
        drive_handshake(conn, &mut ctx),
    )
    .await
}
