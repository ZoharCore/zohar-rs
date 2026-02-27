//! Loading phase handler.
//!
//! Loads player data and transitions to InGame.
//!
//! Flow:
//! 1. Send MainCharacter packet to client
//! 2. Wait for client to send EnterGameRequest
//! 3. Send SetPhaseCommand(Game) and transition to InGame

use super::control::{ControlDecision, handle_session_control};
use super::runtime::{
    PhaseEffects, base_phase_span, disconnect, make_heartbeat_interval, run_phase,
};
use super::session_health::{SessionTick, SessionTracker};
use super::types::{PhaseResult, SessionEnd};
use crate::GameContext;
use crate::adapters::ToProtocol;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};
use zohar_db::{GameDb, PlayersView, ProfilesView, SessionsView};
use zohar_domain::Empire;
use zohar_domain::coords::WorldPos;
use zohar_domain::entity::player::skill::SkillBranch;
use zohar_net::connection::NextConnection;
use zohar_net::connection::game_conn::Loading as ThisPhase;
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::NetId;
use zohar_protocol::game_pkt::loading::{
    LoadingC2s, LoadingC2sSpecific, LoadingS2c, LoadingS2cSpecific,
};

struct LoadingCtx<'a> {
    ctx: Arc<GameContext>,
    handshake: &'a mut zohar_protocol::handshake::HandshakeState,
    session: &'a mut SessionTracker,
    username: String,
    player_name: String,
    player_id: zohar_domain::entity::player::PlayerId,
    net_id: NetId,
    // Player data from DB
    player_class: zohar_domain::entity::player::PlayerClass,
    player_gender: zohar_domain::entity::player::PlayerGender,
    player_empire: Empire,
    spawn_pos: WorldPos,
}

async fn handle_enter(state: &LoadingCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    info!(
        username = %state.username,
        player_id = ?state.player_id,
        spawn_x = state.spawn_pos.x,
        spawn_y = state.spawn_pos.y,
        "Loading player data"
    );

    let mut effects = PhaseEffects::empty();

    // Convert meter float world coords into centimeter i32 world coords
    let (x, y) = state.spawn_pos.to_protocol();

    effects.push(LoadingS2c::Specific(LoadingS2cSpecific::SetMainCharacter {
        net_id: state.net_id,
        class_gender: (state.player_class, state.player_gender).to_protocol(),
        name: state.player_name.clone().into(),
        x,
        y,
        empire: state.player_empire.to_protocol(),
        skill_branch: None::<SkillBranch>.to_protocol(),
    }));
    effects.push(LoadingS2c::Specific(
        LoadingS2cSpecific::SetMainCharacterStats { stats: [0; 255] },
    ));
    Ok(effects)
}

async fn handle_tick(
    now: Instant,
    state: &mut LoadingCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    match state.session.on_tick(now) {
        Some(SessionTick::SendHeartbeat) => {
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
                    "Failed to update session heartbeat during loading"
                );
            }
            Ok(PhaseEffects::send(ControlS2c::RequestHeartbeat.into()))
        }
        Some(SessionTick::TimedOut) => Ok(PhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(PhaseEffects::empty()),
    }
}

async fn handle_packet(
    packet: LoadingC2s,
    now: Instant,
    state: &mut LoadingCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    state.session.mark_rx(now);
    match packet {
        LoadingC2s::Control(control) => {
            match handle_session_control(control, now, state.handshake)? {
                ControlDecision::Handled(outcome) => {
                    let mut effects = PhaseEffects::empty();
                    effects.extend(outcome.send);
                    Ok(effects)
                }
                ControlDecision::Reject(reason) => Ok(PhaseEffects::disconnect(reason)),
            }
        }
        LoadingC2s::Specific(LoadingC2sSpecific::SubmitClientVersion { client, version }) => {
            let client = decode_cstr(&client);
            let version = decode_cstr(&version);
            info!(
                username = %state.username,
                player_id = ?state.player_id,
                client,
                version,
                "Client version"
            );
            Ok(PhaseEffects::empty())
        }
        LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete) => {
            info!(username = %state.username, player_id = ?state.player_id, "Entering game");
            Ok(PhaseEffects::transition(state.net_id))
        }
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<NetId>> {
    for packet in effects.send {
        conn.send(packet).await?;
    }
    if let Some(reason) = effects.disconnect {
        return Err(disconnect(reason));
    }
    Ok(effects.transition)
}

async fn drive_loading(
    mut conn: Connection<ThisPhase>,
    state: &mut LoadingCtx<'_>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    // Enter phase
    let effects = handle_enter(state).await?;
    if let Some(data) = apply_effects(&mut conn, effects).await? {
        return Ok(conn.into_next_with_phase(data).await?);
    }

    let mut heartbeat = make_heartbeat_interval(state.ctx.heartbeat_interval);
    heartbeat.tick().await;

    loop {
        let now = Instant::now();
        let effects = tokio::select! {
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

pub(crate) async fn run_loading(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let username = conn.username().to_string();
    let end_username = username.clone();
    let player_id = conn.player_id();
    let player_name = conn.player_name().to_string();

    // Fetch player data from database
    let player = ctx
        .db
        .players()
        .find_by_id(player_id)
        .await
        .map_err(|_e| SessionEnd::AfterLogin {
            username: username.clone(),
        })?
        .ok_or_else(|| SessionEnd::AfterLogin {
            username: username.clone(),
        })?;

    // Compute spawn position from DB or empire default
    let player_empire = ctx
        .db
        .profiles()
        .find_by_username(&username)
        .await
        .ok()
        .flatten()
        .as_ref()
        .and_then(|profile| profile.empire)
        .expect("need empire for fallback spawn");
    let resolved_spawn = ctx
        .coords
        .resolve_spawn_for_player(Some(&player), player_empire);
    if resolved_spawn.used_fallback {
        warn!(
            username = %username,
            map_key = ?player.map_key,
            local_x = ?player.local_x,
            local_y = ?player.local_y,
            empire = ?player_empire,
            "Falling back to empire start spawn"
        );
    }
    let spawn_pos = ctx
        .coords
        .local_to_world(resolved_spawn.map_id, resolved_spawn.local_pos)
        .expect("resolved local spawn position must map to world coordinates");

    let entity_id = ctx
        .map_events
        .reserve_net_id()
        .await
        .map_err(|_e| SessionEnd::AfterLogin {
            username: username.clone(),
        })?;
    let net_id = entity_id.to_protocol();

    let mut state = LoadingCtx {
        ctx: Arc::clone(ctx),
        handshake,
        session,
        username,
        player_name,
        player_id,
        net_id,
        player_class: player.class,
        player_gender: player.gender,
        player_empire,
        spawn_pos,
    };

    let span = base_phase_span::<ThisPhase>();
    span.record("player", &conn.player_name());
    run_phase(
        "Disconnected during loading",
        SessionEnd::AfterLogin {
            username: end_username,
        },
        span,
        drive_loading(conn, &mut state),
    )
    .await
}
