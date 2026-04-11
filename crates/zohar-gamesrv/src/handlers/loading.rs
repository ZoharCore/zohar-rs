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
    wait_for_server_drain,
};
use super::session_health::{SessionTick, SessionTracker};
use super::types::{PhaseResult, SessionEnd, SessionLeaseAction};
use crate::GameContext;
use crate::adapters::ToProtocol;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};
use zohar_db::{
    GameDb, PlayerStatesView, PlayerStatsBootstrapRow, PlayersView, ProfilesView, SessionsView,
};
use zohar_domain::appearance::PlayerVisualProfile;
use zohar_domain::entity::player::{
    CoreStatAllocations, PlayerGameplayBootstrap, PlayerPlaytime, skill::SkillBranch,
};
use zohar_net::connection::NextConnection;
use zohar_net::connection::game_conn::LoadedPlayer;
use zohar_net::connection::game_conn::Loading as ThisPhase;
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::ingame::stats::{StatsS2c, WireStatSnapshot};
use zohar_protocol::game_pkt::loading::{LoadingC2s, LoadingC2sSpecific, LoadingS2cSpecific};

struct LoadingCtx<'a> {
    ctx: Arc<GameContext>,
    handshake: &'a mut zohar_protocol::handshake::HandshakeState,
    session: &'a mut SessionTracker,
    username: String,
    player_id: zohar_domain::entity::player::PlayerId,
    entry: LoadedPlayer,
    initial_stats: WireStatSnapshot,
}

#[derive(Debug, Clone)]
struct PreparedLoadingStats {
    gameplay: PlayerGameplayBootstrap,
    packet_points: WireStatSnapshot,
}

async fn handle_enter(state: &LoadingCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let spawn_pos = state
        .ctx
        .coords
        .local_to_world(state.entry.map_id, state.entry.initial_pos)
        .expect("resolved local spawn position must map to world coordinates");
    info!(
        username = %state.username,
        player_id = ?state.player_id,
        spawn_x = spawn_pos.x,
        spawn_y = spawn_pos.y,
        "Loading player data"
    );

    let pos = spawn_pos.to_protocol();

    Ok(PhaseEffects::send_many([
        LoadingS2cSpecific::SetMainCharacter {
            net_id: state.entry.net_id,
            class_gender: (
                state.entry.gameplay.class,
                state.entry.visual_profile.gender,
            )
                .to_protocol(),
            name: state.entry.visual_profile.name.clone().into(),
            pos,
            empire: state.entry.visual_profile.empire.to_protocol(),
            skill_branch: None::<SkillBranch>.to_protocol(),
        }
        .into(),
        StatsS2c::SetMainCharacterStats {
            stats: state.initial_stats.clone(),
        }
        .into(),
    ]))
}

fn prepare_loading_stats(
    ctx: &GameContext,
    player: &PlayerStatsBootstrapRow,
) -> Option<PreparedLoadingStats> {
    let gameplay = gameplay_bootstrap_from_row(player);
    let bootstrap = ctx
        .select
        .player_stats
        .hydrate_player(&gameplay)?
        .bootstrap_sync;
    Some(PreparedLoadingStats {
        gameplay,
        packet_points: (&bootstrap.stat_snapshot).to_protocol(),
    })
}

fn gameplay_bootstrap_from_row(player: &PlayerStatsBootstrapRow) -> PlayerGameplayBootstrap {
    PlayerGameplayBootstrap {
        player_id: player.id,
        class: player.class,
        level: player.level,
        exp_in_level: player.exp_in_level,
        core_stat_allocations: CoreStatAllocations {
            allocated_str: player.core_stat_allocations.allocated_str,
            allocated_vit: player.core_stat_allocations.allocated_vit,
            allocated_dex: player.core_stat_allocations.allocated_dex,
            allocated_int: player.core_stat_allocations.allocated_int,
        },
        stat_reset_count: player.stat_reset_count,
        current_hp: player.current_hp,
        current_sp: player.current_sp,
        current_stamina: player.current_stamina,
    }
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
                ControlDecision::Handled(outcome) => Ok(PhaseEffects::send_many(outcome.send)),
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
            Ok(PhaseEffects::transition(state.entry.clone()))
        }
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<LoadedPlayer>> {
    for packet in effects.send {
        conn.send(packet).await?;
    }
    if let Some(error) = effects.disconnect {
        return Err(error);
    }
    Ok(effects.transition)
}

async fn drive_loading(
    mut conn: Connection<ThisPhase>,
    state: &mut LoadingCtx<'_>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    if state.ctx.drain.is_draining() {
        return Err(disconnect("server draining"));
    }

    // Enter phase
    let effects = handle_enter(state).await?;
    if let Some(data) = apply_effects(&mut conn, effects).await? {
        return Ok(conn.into_next_with_phase(data).await?);
    }

    let mut heartbeat = make_heartbeat_interval(state.ctx.heartbeat_interval);
    let mut drain_rx = Some(state.ctx.drain.subscribe());
    heartbeat.tick().await;

    loop {
        let now = Instant::now();
        let effects = tokio::select! {
            _ = wait_for_server_drain(&mut drain_rx) => {
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

pub(crate) async fn run_loading(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    if ctx.drain.is_draining() {
        return Err(SessionEnd::AfterLogin {
            username: conn.username().to_string(),
            lease_action: SessionLeaseAction::Release,
        });
    }

    let username = conn.username().to_string();
    let end_username = username.clone();
    let player_id = conn.player_id();
    let player_name = conn.player_name().to_string();

    // Fetch player data from database
    let players_view = ctx.db.players();
    let runtime_states_view = ctx.db.player_states();
    let (player, runtime_state) = tokio::try_join!(
        players_view.find_stats_bootstrap_by_id(player_id),
        runtime_states_view.find_by_player_id(player_id),
    )
    .map_err(|_e| SessionEnd::AfterLogin {
        username: username.clone(),
        lease_action: SessionLeaseAction::Release,
    })?;
    let player = player.ok_or_else(|| SessionEnd::AfterLogin {
        username: username.clone(),
        lease_action: SessionLeaseAction::Release,
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
        .resolve_spawn_for_player(runtime_state.as_ref(), player_empire);
    if resolved_spawn.used_fallback {
        warn!(
            username = %username,
            map_key = runtime_state.as_ref().and_then(|state| state.map_key.clone()),
            local_x = runtime_state.as_ref().and_then(|state| state.local_x),
            local_y = runtime_state.as_ref().and_then(|state| state.local_y),
            empire = ?player_empire,
            "Falling back to empire start spawn"
        );
    }
    let entity_id = ctx
        .map_events
        .reserve_net_id()
        .await
        .map_err(|_e| SessionEnd::AfterLogin {
            username: username.clone(),
            lease_action: SessionLeaseAction::Release,
        })?;
    let prepared_stats =
        prepare_loading_stats(ctx.as_ref(), &player).ok_or_else(|| SessionEnd::AfterLogin {
            username: username.clone(),
            lease_action: SessionLeaseAction::Release,
        })?;
    let visual_profile = PlayerVisualProfile {
        name: player_name.clone(),
        gender: player.gender,
        empire: player_empire,
        body_part: player.appearance.to_protocol() as u16,
        guild_id: 0,
    };
    let entry = LoadedPlayer {
        net_id: entity_id.to_protocol(),
        map_id: resolved_spawn.map_id,
        runtime_epoch: runtime_state
            .as_ref()
            .map(|state| state.runtime_epoch)
            .unwrap_or_default(),
        playtime: PlayerPlaytime::from_secs_i64(player.playtime_secs),
        initial_pos: resolved_spawn.local_pos,
        visual_profile,
        gameplay: prepared_stats.gameplay.clone(),
    };

    let mut state = LoadingCtx {
        ctx: Arc::clone(ctx),
        handshake,
        session,
        username,
        player_id,
        entry,
        initial_stats: prepared_stats.packet_points,
    };

    let span = base_phase_span::<ThisPhase>();
    span.record("player", conn.player_name());
    run_phase(
        "Disconnected during loading",
        SessionEnd::AfterLogin {
            username: end_username,
            lease_action: SessionLeaseAction::Release,
        },
        span,
        drive_loading(conn, &mut state),
    )
    .await
}
