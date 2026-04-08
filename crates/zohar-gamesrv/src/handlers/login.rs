use crate::adapters::ToProtocol;
use crate::handlers::connection_id_string;
use crate::handlers::control::{ControlDecision, handle_session_control};
use crate::handlers::runtime::{
    PhaseEffects, base_phase_span, disconnect, make_heartbeat_interval, run_phase,
    wait_for_server_drain,
};
use crate::handlers::session_health::{SessionTick, SessionTracker};
use crate::handlers::types::{PhaseResult, SessionEnd};
use crate::{GameContext, GatewayContext, ServerDrainController};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use zohar_db::{DbResult, Game, GameDb, ProfilesView, SessionsView};
use zohar_domain::Empire as DomainEmpire;
use zohar_net::connection::NextConnection;
use zohar_net::connection::game_conn::Login as ThisPhase;
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::login::{
    LoginC2s, LoginC2sSpecific, LoginFailReason, LoginS2cSpecific,
};
use zohar_protocol::token::TokenSigner;

#[derive(Debug, Clone)]
struct TokenLoginInput {
    username: String,
    token: u32,
    enc_key: [u8; 16],
}

#[derive(Debug, Clone)]
enum PersistedCheckMode {
    ClaimActive {
        server_id: String,
        connection_id: String,
        stale_threshold_secs: i64,
    },
    ValidateOnly,
}

#[derive(Debug, Clone)]
enum AuthDecision {
    Accepted {
        username: String,
        empire: Option<DomainEmpire>,
    },
    Rejected {
        reason: LoginFailReason,
    },
}

#[derive(Clone)]
struct LoginDeps {
    db: Game,
    token_signer: Arc<TokenSigner>,
    peer_ip: String,
    idle_ttl_secs: i64,
    mode: PersistedCheckMode,
}

struct LoginCtx<'a> {
    deps: LoginDeps,
    drain: Option<ServerDrainController>,
    heartbeat_interval: Duration,
    handshake: &'a mut zohar_protocol::handshake::HandshakeState,
    session: &'a mut SessionTracker,
}

async fn authenticate_token_login(
    deps: &LoginDeps,
    input: TokenLoginInput,
) -> DbResult<AuthDecision> {
    let accepted = if try_persisted_login(deps, &input).await? {
        true
    } else {
        try_totp_bootstrap(deps, &input).await?
    };

    if !accepted {
        return Ok(AuthDecision::Rejected {
            reason: LoginFailReason::InvalidCredentials,
        });
    }

    let profile = deps.db.profiles().get_or_create(&input.username).await?;
    if profile.is_banned {
        return Ok(AuthDecision::Rejected {
            reason: LoginFailReason::BlockedAccount,
        });
    }

    Ok(AuthDecision::Accepted {
        username: input.username,
        empire: profile.empire,
    })
}

async fn try_persisted_login(deps: &LoginDeps, input: &TokenLoginInput) -> DbResult<bool> {
    match &deps.mode {
        PersistedCheckMode::ClaimActive {
            server_id,
            connection_id,
            stale_threshold_secs,
        } => {
            deps.db
                .sessions()
                .resume_with_token(
                    &input.username,
                    input.token,
                    server_id,
                    connection_id,
                    *stale_threshold_secs,
                    deps.idle_ttl_secs,
                    &deps.peer_ip,
                )
                .await
        }
        PersistedCheckMode::ValidateOnly => {
            deps.db
                .sessions()
                .validate_login_token(
                    &input.username,
                    input.token,
                    deps.idle_ttl_secs,
                    &deps.peer_ip,
                )
                .await
        }
    }
}

async fn try_totp_bootstrap(deps: &LoginDeps, input: &TokenLoginInput) -> DbResult<bool> {
    if !deps
        .token_signer
        .verify(&input.username, input.enc_key, input.token)
    {
        return Ok(false);
    }

    deps.db
        .sessions()
        .set_login_token(&input.username, input.token)
        .await?;

    try_persisted_login(deps, input).await
}

async fn handle_tick(
    now: Instant,
    state: &mut LoginCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    match state.session.on_tick(now) {
        Some(SessionTick::SendHeartbeat) => {
            Ok(PhaseEffects::send(ControlS2c::RequestHeartbeat.into()))
        }
        Some(SessionTick::TimedOut) => Ok(PhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(PhaseEffects::empty()),
    }
}

async fn handle_packet(
    packet: LoginC2s,
    now: Instant,
    state: &mut LoginCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    state.session.mark_rx(now);
    match packet {
        LoginC2s::Control(control) => {
            match handle_session_control(control, now, state.handshake)? {
                ControlDecision::Handled(outcome) => Ok(PhaseEffects::send_many(outcome.send)),
                ControlDecision::Reject(reason) => Ok(PhaseEffects::disconnect(reason)),
            }
        }
        LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin {
            token,
            username,
            enc_key,
        }) => {
            let input = TokenLoginInput {
                username: decode_cstr(&username),
                token,
                enc_key,
            };

            match authenticate_token_login(&state.deps, input).await? {
                AuthDecision::Accepted { username, empire } => {
                    info!(username = %username, "Login accepted via token auth flow");
                    let mut send = Vec::new();
                    if let Some(empire) = empire {
                        send.push(
                            LoginS2cSpecific::SetAccountEmpire {
                                empire: empire.to_protocol(),
                            }
                            .into(),
                        );
                    }
                    Ok(PhaseEffects {
                        send,
                        transition: Some(username),
                        disconnect: None,
                    })
                }
                AuthDecision::Rejected { reason } => {
                    warn!("Missing or invalid login token");
                    Ok(
                        PhaseEffects::send(LoginS2cSpecific::LoginResultFail { reason }.into())
                            .with_disconnect("invalid login key"),
                    )
                }
            }
        }
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<String>> {
    for packet in effects.send {
        conn.send(packet).await?;
    }
    if let Some(error) = effects.disconnect {
        return Err(error);
    }
    Ok(effects.transition)
}

async fn drive_login(
    mut conn: Connection<ThisPhase>,
    state: &mut LoginCtx<'_>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    if state
        .drain
        .as_ref()
        .is_some_and(ServerDrainController::is_draining)
    {
        return Err(disconnect("server draining"));
    }

    let mut heartbeat = make_heartbeat_interval(state.heartbeat_interval);
    let mut drain_rx = state.drain.as_ref().map(ServerDrainController::subscribe);
    let drain_enabled = drain_rx.is_some();
    heartbeat.tick().await;

    loop {
        let now = Instant::now();
        let effects = tokio::select! {
            _ = wait_for_server_drain(&mut drain_rx), if drain_enabled => {
                PhaseEffects::disconnect("server draining")
            }
            _ = heartbeat.tick() => handle_tick(now, state).await?,
            packet = conn.recv() => {
                let packet = packet?.ok_or_else(|| disconnect("connection closed"))?;
                handle_packet(packet, now, state).await?
            }
        };

        if let Some(data) = apply_effects(&mut conn, effects).await? {
            return Ok(conn.into_next_with_phase(data).await?);
        }
    }
}

pub(crate) async fn run_login_core(
    conn_id: Uuid,
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let peer_ip = match conn.peer_ip_string() {
        Ok(ip) => ip,
        Err(error) => {
            warn!(error = %error, "Failed to read peer IP during login");
            return Err(SessionEnd::BeforeLogin);
        }
    };

    let mut state = LoginCtx {
        deps: LoginDeps {
            db: ctx.db.clone(),
            token_signer: ctx.token_signer.clone(),
            peer_ip,
            idle_ttl_secs: ctx.login_token_idle_ttl.as_secs() as i64,
            mode: PersistedCheckMode::ClaimActive {
                server_id: ctx.server_id.clone(),
                connection_id: connection_id_string(conn_id),
                stale_threshold_secs: ctx.active_session_stale_threshold.as_secs() as i64,
            },
        },
        drain: Some(ctx.drain.clone()),
        heartbeat_interval: ctx.heartbeat_interval,
        handshake,
        session,
    };

    let span = base_phase_span::<ThisPhase>();
    run_phase(
        "Disconnected during login",
        SessionEnd::BeforeLogin,
        span,
        drive_login(conn, &mut state),
    )
    .await
}

pub(crate) async fn run_login_gateway(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GatewayContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let peer_ip = match conn.peer_ip_string() {
        Ok(ip) => ip,
        Err(error) => {
            warn!(error = %error, "Failed to read peer IP during login");
            return Err(SessionEnd::BeforeLogin);
        }
    };

    let mut state = LoginCtx {
        deps: LoginDeps {
            db: ctx.db.clone(),
            token_signer: ctx.token_signer.clone(),
            peer_ip,
            idle_ttl_secs: ctx.login_token_idle_ttl.as_secs() as i64,
            mode: PersistedCheckMode::ValidateOnly,
        },
        drain: None,
        heartbeat_interval: ctx.heartbeat_interval,
        handshake,
        session,
    };
    let span = base_phase_span::<ThisPhase>();
    run_phase(
        "Disconnected during login",
        SessionEnd::BeforeLogin,
        span,
        drive_login(conn, &mut state),
    )
    .await
}
