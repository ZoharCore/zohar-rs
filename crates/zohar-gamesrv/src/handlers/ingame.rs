//! In-game phase handler.
//!
//! Main game loop - handles movement, combat, chat, etc.

use super::connection_id_string;
use super::control::{ControlDecision, handle_session_control};
use super::runtime::{
    PhaseEffects, base_phase_span, disconnect, make_heartbeat_interval, run_phase,
    wait_for_server_drain,
};
use super::session_health::{SessionTick, SessionTracker};
use super::types::{PhaseResult, SessionEnd, SessionLeaseAction};
use crate::handlers::ingame::relocation::handle_portal_entry;
use crate::{GameContext, SERVER_DRAIN_GRACE_PERIOD};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::time::Instant as TokioInstant;
use tracing::warn;
use uuid::Uuid;
use zohar_db::{GameDb, SessionsView};
use zohar_domain::{Empire as DomainEmpire, MapId, PlayerExitKind, entity::player::PlayerSnapshot};
use zohar_map_port::{EnterMsg, LeaveMsg, PlayerEvent};
use zohar_net::{Connection, ConnectionPhaseExt, connection::NextConnection};
use zohar_protocol::{
    game_pkt::{
        ControlS2c,
        ingame::{InGameC2s, InGameS2c, system::SystemS2c},
    },
    handshake::HandshakeState,
};
use zohar_sim::{PlayerPersistenceQueueError, PlayerPersistenceResult};

pub(super) mod chat;
pub(super) mod combat;
pub(super) mod fishing;
pub(super) mod guild;
pub(super) mod movement;
pub(super) mod relocation;
pub(super) mod stats;
pub(super) mod trading;
pub(super) mod world;

pub(super) type ThisPhase = zohar_net::connection::game_conn::InGame;
pub(super) type InGamePhaseEffects = PhaseEffects<ThisPhase>;

pub(super) struct InGameCtx<'a> {
    ctx: Arc<GameContext>,
    handshake: &'a mut HandshakeState,
    session: &'a mut SessionTracker,
    pub(super) username: String,
    pub(super) connection_id: String,
    pub(super) player_name: String,
    pub(super) player_id: zohar_domain::entity::player::PlayerId,
    pub(super) player_net_id: zohar_domain::entity::EntityId,
    pub(super) map_id: MapId,
    pub(super) player_empire: DomainEmpire,
}

struct PreparedInGame<'a> {
    conn: Connection<ThisPhase>,
    state: InGameCtx<'a>,
    map_rx: tokio::sync::mpsc::Receiver<PlayerEvent>,
    source_map_leave: Option<LeaveMsg>,
}

const MAP_EVENT_BURST_LIMIT: usize = 32;
const PLAYER_PERSISTENCE_TIMEOUT: Duration = Duration::from_secs(3);

fn player_persistence_timeout(ctx: &GameContext) -> Duration {
    if ctx.drain.is_draining() {
        SERVER_DRAIN_GRACE_PERIOD
    } else {
        PLAYER_PERSISTENCE_TIMEOUT
    }
}

async fn await_persistence_result(
    reply_rx: oneshot::Receiver<PlayerPersistenceResult>,
    deadline: TokioInstant,
    op_name: &'static str,
) -> anyhow::Result<()> {
    match tokio::time::timeout_at(deadline, reply_rx).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(anyhow::anyhow!("{op_name} failed: {error}")),
        Ok(Err(_)) => Err(anyhow::anyhow!("{op_name} reply channel dropped")),
        Err(_) => Err(anyhow::anyhow!("{op_name} timed out")),
    }
}

async fn run_persistence_op<F>(
    enqueue: F,
    timeout: Duration,
    op_name: &'static str,
) -> anyhow::Result<()>
where
    F: Future<
        Output = Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError>,
    >,
{
    let deadline = TokioInstant::now() + timeout;
    let reply_rx = match tokio::time::timeout_at(deadline, enqueue).await {
        Ok(Ok(reply_rx)) => reply_rx,
        Ok(Err(error)) => {
            return Err(anyhow::anyhow!("failed to enqueue {op_name}: {error}"));
        }
        Err(_) => {
            return Err(anyhow::anyhow!(
                "{op_name} timed out while waiting for queue capacity"
            ));
        }
    };

    await_persistence_result(reply_rx, deadline, op_name).await
}

async fn flush_player_state(ctx: &GameContext, snapshot: PlayerSnapshot) -> anyhow::Result<()> {
    run_persistence_op(
        ctx.player_persistence.schedule_flush(snapshot),
        player_persistence_timeout(ctx),
        "player state flush",
    )
    .await
}

async fn commit_player_exit(
    ctx: &GameContext,
    exit_kind: PlayerExitKind,
    username: &str,
    connection_id: &str,
    snapshot: PlayerSnapshot,
) -> anyhow::Result<()> {
    run_persistence_op(
        ctx.player_persistence.commit_player_exit(
            exit_kind,
            username,
            ctx.server_id.clone(),
            connection_id.to_string(),
            snapshot,
        ),
        player_persistence_timeout(ctx),
        match exit_kind {
            PlayerExitKind::Disconnect => "player disconnect finalization",
            PlayerExitKind::Handoff => "player handoff preparation",
        },
    )
    .await
}

fn enter_packets(state: &mut InGameCtx<'_>) -> Vec<InGameS2c> {
    let now = Instant::now();
    vec![
        SystemS2c::SetServerTime {
            time: state.handshake.uptime_at(now).into(),
        }
        .into(),
        SystemS2c::SetChannelInfo {
            channel_id: state.ctx.channel_id.min(u8::MAX as u32) as u8,
        }
        .into(),
        ControlS2c::RequestHandshake {
            data: state.handshake.sync_data(now, Duration::ZERO),
        }
        .into(),
    ]
}

async fn handle_session_tick(state: &mut InGameCtx<'_>) -> PhaseResult<InGamePhaseEffects> {
    let now = Instant::now();
    match state.session.on_tick(now) {
        Some(SessionTick::SendHeartbeat) => {
            // Keep active-session liveness on a coarse cadence, not per gameplay packet.
            if let Err(error) = state
                .ctx
                .db
                .sessions()
                .update_heartbeat(&state.username)
                .await
            {
                warn!(
                    username = %state.username,
                    error = ?error,
                    "Failed to update session heartbeat"
                );
            }
            Ok(InGamePhaseEffects::send_many([
                ControlS2c::RequestHeartbeat.into(),
                SystemS2c::SetServerTime {
                    time: state.handshake.uptime_at(now).into(),
                }
                .into(),
            ]))
        }
        Some(SessionTick::TimedOut) => Ok(InGamePhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(InGamePhaseEffects::empty()),
    }
}

async fn handle_packet(
    packet: InGameC2s,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    let now = Instant::now();
    state.session.mark_rx(now);
    match packet {
        InGameC2s::Control(packet) => match handle_session_control(packet, now, state.handshake)? {
            ControlDecision::Handled(outcome) => Ok(InGamePhaseEffects::send_many(outcome.send)),
            ControlDecision::Reject(reason) => Ok(InGamePhaseEffects::disconnect(reason)),
        },
        InGameC2s::Chat(packet) => chat::handle_packet(packet, state).await,
        InGameC2s::Combat(packet) => combat::handle_packet(packet, state).await,
        InGameC2s::Move(packet) => movement::handle_packet(packet, state).await,
        InGameC2s::Trading(packet) => trading::handle_packet(packet, state).await,
        InGameC2s::Guild(packet) => guild::handle_packet(packet, state).await,
        InGameC2s::Fishing(packet) => fishing::handle_packet(packet, state).await,
    }
}

fn map_event_to_packets(
    event: PlayerEvent,
    map_id: MapId,
    coords: &crate::ContentCoords,
) -> Vec<InGameS2c> {
    match event {
        PlayerEvent::EntitySpawn { show, details } => {
            world::encode_entity_spawn(show, details, map_id, coords)
        }
        PlayerEvent::SetEntityStat {
            entity_id,
            stat,
            delta,
            absolute,
        } => stats::encode_entity_stat(entity_id, stat, delta, absolute),
        PlayerEvent::EntityMove(event) => movement::encode_entity_move(event, map_id, coords),
        PlayerEvent::SetEntityMovementAnimation {
            entity_id,
            animation,
        } => movement::encode_entity_movement_animation(entity_id, animation),
        PlayerEvent::EntityDespawn { entity_id } => world::encode_entity_despawn(entity_id),
        PlayerEvent::Chat {
            channel,
            sender_entity_id,
            empire,
            message,
        } => chat::encode_chat_event(channel, sender_entity_id, empire, message),
        PlayerEvent::PortalEntered { .. } => Vec::new(),
    }
}

async fn map_event_to_effects(
    event: PlayerEvent,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    match event {
        PlayerEvent::PortalEntered { destination } => handle_portal_entry(state, destination).await,
        other => Ok(InGamePhaseEffects::send_many(map_event_to_packets(
            other,
            state.map_id,
            state.ctx.coords.as_ref(),
        ))),
    }
}

async fn apply_runtime_effects(
    conn: &mut Connection<ThisPhase>,
    effects: InGamePhaseEffects,
) -> PhaseResult<Option<()>> {
    for packet in effects.send {
        send_outbound_packet(conn, packet).await?;
    }
    if let Some(error) = effects.disconnect {
        return Err(error);
    }
    Ok(effects.transition)
}

async fn send_outbound_packet(
    conn: &mut Connection<ThisPhase>,
    packet: InGameS2c,
) -> PhaseResult<()> {
    conn.send(packet).await?;
    Ok(())
}

async fn drain_outbound_burst(
    conn: &mut Connection<ThisPhase>,
    state: &mut InGameCtx<'_>,
    map_rx: &mut tokio::sync::mpsc::Receiver<PlayerEvent>,
    max_events: usize,
) -> PhaseResult<()> {
    for _ in 0..max_events {
        let Ok(event) = map_rx.try_recv() else {
            break;
        };
        let effects = map_event_to_effects(event, state).await?;
        let _ = apply_runtime_effects(conn, effects).await?;
    }
    Ok(())
}

async fn drive_ingame(
    mut conn: Connection<ThisPhase>,
    state: &mut InGameCtx<'_>,
    mut map_rx: tokio::sync::mpsc::Receiver<PlayerEvent>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    if state.ctx.drain.is_draining() {
        return Err(disconnect("server draining"));
    }

    for packet in enter_packets(state) {
        send_outbound_packet(&mut conn, packet).await?;
    }

    let mut heartbeat = make_heartbeat_interval(state.ctx.heartbeat_interval);
    let mut drain_rx = Some(state.ctx.drain.subscribe());
    heartbeat.tick().await;

    loop {
        // Keep outbound map traffic progressing even when inbound client traffic is
        // continuously ready, preventing observer movement starvation.
        drain_outbound_burst(&mut conn, state, &mut map_rx, MAP_EVENT_BURST_LIMIT).await?;

        let effects = tokio::select! {
            _ = wait_for_server_drain(&mut drain_rx) => {
                InGamePhaseEffects::disconnect("server draining")
            }
            _ = heartbeat.tick() => {
                handle_session_tick(state).await?
            },
            packet = conn.recv() => {
                let packet = packet?.ok_or_else(|| disconnect("connection closed"))?;
                handle_packet(packet, state).await?
            }
            outbound = map_rx.recv() => {
                if let Some(event) = outbound {
                    let effects = map_event_to_effects(event, state).await?;
                    let _ = apply_runtime_effects(&mut conn, effects).await?;
                }
                continue;
            }
        };

        if let Some(data) = apply_runtime_effects(&mut conn, effects).await? {
            return Ok(conn.into_next_with_phase(data).await?);
        }
    }
}

fn prepare_ingame<'a>(
    conn_id: Uuid,
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &'a mut HandshakeState,
    session: &'a mut SessionTracker,
) -> Result<PreparedInGame<'a>, SessionEnd> {
    let username = conn.username().to_string();

    if ctx.drain.is_draining() {
        return Err(SessionEnd::AfterLogin {
            username,
            lease_action: SessionLeaseAction::Release,
        });
    }

    let entry = conn.entry().clone();
    let player_name = conn.player_name().to_string();
    let player_id = conn.player_id();
    let player_net_id = zohar_domain::entity::EntityId(entry.net_id.into());
    let map_id = entry.map_id;
    let player_empire = entry.visual_profile.empire;

    let Some(map_code) = ctx.coords.map_code_by_id(map_id) else {
        return Err(SessionEnd::AfterLogin {
            username,
            lease_action: SessionLeaseAction::Release,
        });
    };

    // Validate we landed on the correct map core. Endpoint equality is not stable across
    // exposure modes (e.g. Agones hostPort vs NodePort fronted Service).
    if map_code != ctx.map_code {
        warn!(
            username = %username,
            player_map = %map_code,
            core_map = %ctx.map_code,
            channel_id = ctx.channel_id,
            "Player connected to wrong map core"
        );
        return Err(SessionEnd::AfterLogin {
            username,
            lease_action: SessionLeaseAction::Release,
        });
    }

    let (map_rx, entered_map) = match ctx.map_events.enter_player(EnterMsg {
        player_id,
        player_net_id,
        runtime_epoch: entry.runtime_epoch,
        playtime: entry.playtime,
        initial_pos: entry.initial_pos,
        visual_profile: entry.visual_profile.clone(),
        gameplay: entry.gameplay.clone(),
    }) {
        Ok(map_rx) => (map_rx, true),
        Err(err) => {
            warn!(error = ?err, "Failed to register player with map runtime");
            let (_tx, map_rx) = tokio::sync::mpsc::channel(1);
            (map_rx, false)
        }
    };

    Ok(PreparedInGame {
        conn,
        state: InGameCtx {
            ctx: Arc::clone(ctx),
            handshake,
            session,
            username,
            connection_id: connection_id_string(conn_id),
            player_name,
            player_id,
            player_net_id,
            map_id,
            player_empire,
        },
        map_rx,
        source_map_leave: entered_map.then_some(LeaveMsg {
            player_id,
            player_net_id,
        }),
    })
}

async fn finalize_ingame_result(
    state: &InGameCtx<'_>,
    source_map_leave: Option<LeaveMsg>,
    result: Result<NextConnection<ThisPhase>, SessionEnd>,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let Some(leave_msg) = source_map_leave else {
        return result;
    };

    match result {
        Ok(conn_next) => {
            let snapshot = match state
                .ctx
                .map_events
                .leave_player_and_snapshot(leave_msg)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    warn!(
                        username = %state.username,
                        player_id = ?state.player_id,
                        error = %error,
                        "Failed to leave map and capture player snapshot during phase-select transition"
                    );
                    return Err(SessionEnd::AfterLogin {
                        username: state.username.clone(),
                        lease_action: SessionLeaseAction::Release,
                    });
                }
            };

            match flush_player_state(&state.ctx, snapshot.clone()).await {
                Ok(()) => Ok(conn_next),
                Err(flush_error) => {
                    warn!(
                        username = %state.username,
                        player_id = ?state.player_id,
                        error = %flush_error,
                        "Player snapshot flush failed during phase-select transition; finalizing disconnect instead"
                    );
                    match commit_player_exit(
                        &state.ctx,
                        PlayerExitKind::Disconnect,
                        &state.username,
                        &state.connection_id,
                        snapshot,
                    )
                    .await
                    {
                        Ok(()) => Err(SessionEnd::AfterLogin {
                            username: state.username.clone(),
                            lease_action: SessionLeaseAction::AlreadyReleased,
                        }),
                        Err(finalize_error) => {
                            warn!(
                                username = %state.username,
                                player_id = ?state.player_id,
                                error = %finalize_error,
                                "Transactional disconnect finalization failed after phase-select flush error"
                            );
                            Err({
                                SessionEnd::AfterLogin {
                                    username: state.username.clone(),
                                    lease_action: SessionLeaseAction::RetainUntilStale,
                                }
                            })
                        }
                    }
                }
            }
        }
        Err(SessionEnd::Handoff { username }) => {
            if let Err(error) = state.ctx.map_events.send_player_leave(leave_msg) {
                warn!(
                    username = %state.username,
                    player_id = ?state.player_id,
                    error = %error,
                    "Failed to remove player from source map after handoff preparation"
                );
            }
            Err(SessionEnd::Handoff { username })
        }
        Err(SessionEnd::AfterLogin { username, .. }) => {
            let lease_action = match state
                .ctx
                .map_events
                .leave_player_and_snapshot(leave_msg)
                .await
            {
                Ok(snapshot) => match commit_player_exit(
                    &state.ctx,
                    PlayerExitKind::Disconnect,
                    &username,
                    &state.connection_id,
                    snapshot,
                )
                .await
                {
                    Ok(()) => SessionLeaseAction::AlreadyReleased,
                    Err(error) => {
                        warn!(
                            username = %username,
                            player_id = ?state.player_id,
                            error = %error,
                            "Transactional disconnect finalization failed after player left the map"
                        );
                        SessionLeaseAction::RetainUntilStale
                    }
                },
                Err(error) => {
                    warn!(
                        username = %username,
                        player_id = ?state.player_id,
                        error = %error,
                        "Failed to leave map and capture player snapshot while disconnecting player"
                    );
                    SessionLeaseAction::Release
                }
            };

            Err(SessionEnd::AfterLogin {
                username,
                lease_action,
            })
        }
        Err(SessionEnd::BeforeLogin) => Err(SessionEnd::BeforeLogin),
    }
}

pub(crate) async fn run_ingame(
    conn_id: Uuid,
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let PreparedInGame {
        conn,
        mut state,
        map_rx,
        source_map_leave,
    } = prepare_ingame(conn_id, conn, ctx, handshake, session)?;

    let span = base_phase_span::<ThisPhase>();
    span.record("player", conn.player_name());

    let result = run_phase(
        "Player disconnected from game",
        SessionEnd::AfterLogin {
            username: state.username.clone(),
            lease_action: SessionLeaseAction::Release,
        },
        span,
        drive_ingame(conn, &mut state, map_rx),
    )
    .await;

    finalize_ingame_result(&state, source_map_leave, result).await
}

#[cfg(test)]
mod tests {
    use super::run_persistence_op;
    use std::future::pending;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn unbiased_select_eventually_services_outbound_under_inbound_pressure() {
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<()>();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<()>();

        for _ in 0..256 {
            inbound_tx
                .send(())
                .expect("seed inbound backlog for starvation probe");
        }
        outbound_tx
            .send(())
            .expect("seed outbound packet for starvation probe");

        let mut saw_outbound = false;
        for _ in 0..512 {
            tokio::select! {
                Some(()) = inbound_rx.recv() => {
                    // Keep inbound perpetually ready to emulate a spammy client.
                    inbound_tx.send(()).expect("refill inbound backlog");
                }
                Some(()) = outbound_rx.recv() => {
                    saw_outbound = true;
                    break;
                }
            }
        }

        assert!(
            saw_outbound,
            "outbound work must make progress even when inbound is continuously ready"
        );
    }

    #[tokio::test]
    async fn persistence_timeout_covers_queue_admission() {
        let error = run_persistence_op(
            pending(),
            Duration::from_millis(20),
            "player snapshot flush",
        )
        .await
        .expect_err("stalled queue admission should time out");

        assert!(
            error
                .to_string()
                .contains("timed out while waiting for queue capacity"),
            "unexpected error: {error}"
        );
    }
}
