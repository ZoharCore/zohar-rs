//! Character select phase handler.
//!
//! Handles empire selection, character create/delete/select.

use super::control::{ControlDecision, handle_session_control};
use super::runtime::{
    PhaseEffects, base_phase_span, disconnect, make_heartbeat_interval, run_phase,
    wait_for_server_drain,
};
use super::session_health::{SessionTick, SessionTracker};
use super::types::{PhaseResult, SessionEnd, SessionLeaseAction};
use crate::adapters::{PlayerEndpoint, ToDomain, ToProtocol, ToProtocolPlayer};
use crate::infra::MapEndpointResolver;
use crate::{ContentCoords, EmpireStartMaps, GameContext, GatewayContext, ServerDrainController};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};
use zohar_db::{CreatePlayerOutcome, Game, GameDb, PlayersView, ProfilesView, SessionsView};
use zohar_domain::Empire as DomainEmpire;
use zohar_net::connection::NextConnection;
use zohar_net::connection::game_conn::{Select as ThisPhase, SelectedPlayer};
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::select::{
    CreatePlayerError, GuildName, MAX_PLAYER_SLOTS, Player, PlayerSelectSlot, SelectC2s,
    SelectC2sSpecific, SelectS2c, SelectS2cSpecific,
};

const NEW_PLAYER_NAME_VALID_LEN: std::ops::RangeInclusive<usize> = 2..=16; // TODO: configurable

struct SelectCtx<'a> {
    runtime: SelectRuntime,
    mode: SelectMode,
    handshake: &'a mut zohar_protocol::handshake::HandshakeState,
    session: &'a mut SessionTracker,
    username: String,
}

#[derive(Clone)]
struct SelectRuntime {
    db: Game,
    routing: SelectRouting,
    drain: Option<ServerDrainController>,
    heartbeat_interval: std::time::Duration,
    channel_id: u32,
    map_resolver: Arc<MapEndpointResolver>,
}

#[derive(Clone)]
enum SelectRouting {
    Core { coords: Arc<ContentCoords> },
    Gateway { empire_start_maps: EmpireStartMaps },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectMode {
    CoreTransition,
    GatewayBrowseOnly,
}

async fn handle_enter(state: &mut SelectCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let players = state
        .runtime
        .db
        .players()
        .list_for_user(&state.username)
        .await?;
    Ok(PhaseEffects::send(
        build_players_pkt(&players, state).await?,
    ))
}

async fn handle_tick(
    now: Instant,
    state: &mut SelectCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    match state.session.on_tick(now) {
        Some(SessionTick::SendHeartbeat) => {
            if let Err(error) = state
                .runtime
                .db
                .sessions()
                .update_heartbeat(&state.username)
                .await
            {
                warn!(
                    username = %state.username,
                    error = ?error,
                    "Failed to update session heartbeat during select"
                );
            }
            Ok(PhaseEffects::send(ControlS2c::RequestHeartbeat.into()))
        }
        Some(SessionTick::TimedOut) => Ok(PhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(PhaseEffects::empty()),
    }
}

async fn handle_packet(
    packet: SelectC2s,
    now: Instant,
    state: &mut SelectCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    state.session.mark_rx(now);
    match packet {
        SelectC2s::Control(control) => match handle_session_control(control, now, state.handshake)?
        {
            ControlDecision::Handled(outcome) => Ok(PhaseEffects::send_many(outcome.send)),
            ControlDecision::Reject(reason) => Ok(PhaseEffects::disconnect(reason)),
        },
        SelectC2s::Specific(SelectC2sSpecific::SubmitEmpireChoice { empire }) => {
            let empire = empire.to_domain();
            state
                .runtime
                .db
                .profiles()
                .update_empire(&state.username, empire)
                .await?;
            Ok(PhaseEffects::send(
                SelectS2cSpecific::SetAccountEmpire {
                    empire: empire.to_protocol(),
                }
                .into(),
            ))
        }
        SelectC2s::Specific(SelectC2sSpecific::RequestCreatePlayer {
            slot,
            name,
            class_gender,
            appearance,
            stat_vit,
            stat_int,
            stat_str,
            stat_dex,
            ..
        }) => {
            let name = decode_cstr(&name);
            if !NEW_PLAYER_NAME_VALID_LEN.contains(&name.len()) {
                warn!(name = %name, "Invalid player name length");
                return Ok(PhaseEffects::send(
                    SelectS2cSpecific::CreatePlayerResultFail {
                        error: CreatePlayerError::GenericFailure,
                    }
                    .into(),
                ));
            }

            let slot_index: u8 = slot.into();
            if slot_index >= MAX_PLAYER_SLOTS as u8 {
                return Ok(PhaseEffects::send(
                    SelectS2cSpecific::CreatePlayerResultFail {
                        error: CreatePlayerError::GenericFailure,
                    }
                    .into(),
                ));
            }

            let stat_str: u8 = match stat_str.try_into() {
                Ok(value) => value,
                Err(_) => {
                    return Ok(PhaseEffects::send(
                        SelectS2cSpecific::CreatePlayerResultFail {
                            error: CreatePlayerError::GenericFailure,
                        }
                        .into(),
                    ));
                }
            };
            let stat_vit: u8 = match stat_vit.try_into() {
                Ok(value) => value,
                Err(_) => {
                    return Ok(PhaseEffects::send(
                        SelectS2cSpecific::CreatePlayerResultFail {
                            error: CreatePlayerError::GenericFailure,
                        }
                        .into(),
                    ));
                }
            };
            let stat_dex: u8 = match stat_dex.try_into() {
                Ok(value) => value,
                Err(_) => {
                    return Ok(PhaseEffects::send(
                        SelectS2cSpecific::CreatePlayerResultFail {
                            error: CreatePlayerError::GenericFailure,
                        }
                        .into(),
                    ));
                }
            };
            let stat_int: u8 = match stat_int.try_into() {
                Ok(value) => value,
                Err(_) => {
                    return Ok(PhaseEffects::send(
                        SelectS2cSpecific::CreatePlayerResultFail {
                            error: CreatePlayerError::GenericFailure,
                        }
                        .into(),
                    ));
                }
            };

            let (class, gender) = class_gender.to_domain();
            let outcome = state
                .runtime
                .db
                .players()
                .create(
                    &state.username,
                    slot_index.into(),
                    &name,
                    class,
                    gender,
                    appearance.to_domain(),
                    stat_str,
                    stat_vit,
                    stat_dex,
                    stat_int,
                )
                .await?;

            match outcome {
                CreatePlayerOutcome::Created(player) => {
                    let slot = match PlayerSelectSlot::try_from(slot_index) {
                        Ok(slot) => slot,
                        Err(_) => {
                            warn!(slot_index, "Invalid player slot from create outcome");
                            return Ok(PhaseEffects::disconnect(
                                "invalid player slot from create outcome",
                            ));
                        }
                    };
                    let endpoint = match resolve_player_endpoint(&player, state).await {
                        Ok(endpoint) => endpoint,
                        Err(error) => {
                            warn!(
                                error = ?error,
                                player_id = ?player.id,
                                slot = slot_index,
                                "Character map routing unavailable after create; advertising unroutable endpoint"
                            );
                            unroutable_endpoint()
                        }
                    };
                    Ok(PhaseEffects::send(
                        SelectS2cSpecific::CreatePlayerResultOk {
                            slot,
                            new_player: player.to_domain().to_protocol_player(endpoint),
                        }
                        .into(),
                    ))
                }
                CreatePlayerOutcome::NameTaken => Ok(PhaseEffects::send(
                    SelectS2cSpecific::CreatePlayerResultFail {
                        error: CreatePlayerError::NameAlreadyExists,
                    }
                    .into(),
                )),
            }
        }
        SelectC2s::Specific(SelectC2sSpecific::RequestDeletePlayer { slot, code, .. }) => {
            let slot_index: u8 = slot.into();
            if slot_index >= 4 {
                warn!(slot_index, "Player slot out of range");
                return Ok(PhaseEffects::send(
                    SelectS2cSpecific::DeletePlayerResultFail.into(),
                ));
            }

            let provided_code = decode_cstr(&code);
            let deleted = state
                .runtime
                .db
                .players()
                .delete_with_code(&state.username, slot_index, &provided_code)
                .await?;

            if deleted {
                let slot = match PlayerSelectSlot::try_from(slot_index) {
                    Ok(slot) => slot,
                    Err(_) => {
                        warn!(slot_index, "Invalid player slot from delete outcome");
                        return Ok(PhaseEffects::disconnect(
                            "invalid player slot from delete outcome",
                        ));
                    }
                };
                Ok(PhaseEffects::send(
                    SelectS2cSpecific::DeletePlayerResultOk { slot }.into(),
                ))
            } else {
                info!(username = %state.username, slot_index, "Delete player failed");
                Ok(PhaseEffects::send(
                    SelectS2cSpecific::DeletePlayerResultFail.into(),
                ))
            }
        }
        SelectC2s::Specific(SelectC2sSpecific::SubmitPlayerChoice { slot }) => {
            if state.mode != SelectMode::CoreTransition {
                return Ok(PhaseEffects::disconnect(
                    "submit player choice is only supported in core mode",
                ));
            }
            let slot_index: u8 = slot.into();
            let player = state
                .runtime
                .db
                .players()
                .find_by_slot(&state.username, slot_index)
                .await?;

            match player {
                Some(player) => {
                    info!(username = %state.username, player_id = ?player.id, "Player selected");
                    Ok(PhaseEffects::transition(SelectedPlayer {
                        player_id: player.id,
                        player_name: player.name,
                    }))
                }
                None => {
                    warn!("Player slot empty or out of range");
                    Ok(PhaseEffects::empty())
                }
            }
        }
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<SelectedPlayer>> {
    for packet in effects.send {
        conn.send(packet).await?;
    }
    if let Some(reason) = effects.disconnect {
        return Err(disconnect(reason));
    }
    Ok(effects.transition)
}

async fn drive_select(
    mut conn: Connection<ThisPhase>,
    state: &mut SelectCtx<'_>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    if state
        .runtime
        .drain
        .as_ref()
        .is_some_and(ServerDrainController::is_draining)
    {
        return Err(disconnect("server draining"));
    }

    // Enter phase
    let effects = handle_enter(state).await?;
    if let Some(data) = apply_effects(&mut conn, effects).await? {
        return Ok(conn.into_next_with_phase(data).await?);
    }

    let mut heartbeat = make_heartbeat_interval(state.runtime.heartbeat_interval);
    let mut drain_rx = state
        .runtime
        .drain
        .as_ref()
        .map(ServerDrainController::subscribe);
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

pub(crate) async fn run_select_core(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    let username = conn.username().to_string();
    let end_username = username.clone();
    let mut state = SelectCtx {
        runtime: SelectRuntime {
            db: ctx.db.clone(),
            routing: SelectRouting::Core {
                coords: Arc::clone(&ctx.coords),
            },
            drain: Some(ctx.drain.clone()),
            heartbeat_interval: ctx.heartbeat_interval,
            channel_id: ctx.channel_id,
            map_resolver: Arc::clone(&ctx.map_resolver),
        },
        mode: SelectMode::CoreTransition,
        handshake,
        session,
        username,
    };
    let span = base_phase_span::<ThisPhase>();
    span.record("username", &state.username);
    run_phase(
        "Disconnected during select",
        SessionEnd::AfterLogin {
            username: end_username,
            lease_action: SessionLeaseAction::Release,
        },
        span,
        drive_select(conn, &mut state),
    )
    .await
}

pub(crate) async fn run_select_gateway(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GatewayContext>,
    handshake: &mut zohar_protocol::handshake::HandshakeState,
    session: &mut SessionTracker,
) -> Result<Infallible, SessionEnd> {
    let username = conn.username().to_string();
    let end_username = username.clone();
    let mut state = SelectCtx {
        runtime: SelectRuntime {
            db: ctx.db.clone(),
            routing: SelectRouting::Gateway {
                empire_start_maps: ctx.empire_start_maps.clone(),
            },
            drain: None,
            heartbeat_interval: ctx.heartbeat_interval,
            channel_id: ctx.channel_id,
            map_resolver: Arc::clone(&ctx.map_resolver),
        },
        mode: SelectMode::GatewayBrowseOnly,
        handshake,
        session,
        username,
    };
    let span = base_phase_span::<ThisPhase>();
    span.record("username", &state.username);
    let result = run_phase(
        "Disconnected during select",
        SessionEnd::AfterLogin {
            username: end_username,
            lease_action: SessionLeaseAction::Release,
        },
        span,
        drive_select(conn, &mut state),
    )
    .await;

    match result {
        Ok(_unexpected) => Err(SessionEnd::AfterLogin {
            username: state.username.clone(),
            lease_action: SessionLeaseAction::Release,
        }),
        Err(end) => Err(end),
    }
}

async fn build_players_pkt(
    db_players: &[zohar_db::PlayerRow],
    state: &SelectCtx<'_>,
) -> PhaseResult<SelectS2c> {
    let players: [Player; MAX_PLAYER_SLOTS] = std::array::from_fn(|_| Player::empty());
    let mut players = players;

    for slot in 0..MAX_PLAYER_SLOTS {
        let Some(db_player) = db_players.iter().find(|p| p.slot as usize == slot) else {
            continue;
        };
        let endpoint = match resolve_player_endpoint(db_player, state).await {
            Ok(endpoint) => endpoint,
            Err(error) => {
                warn!(
                    error = ?error,
                    player_id = ?db_player.id,
                    slot = db_player.slot,
                    "Character map routing unavailable; advertising unroutable endpoint for slot"
                );
                unroutable_endpoint()
            }
        };
        players[slot] = db_player.to_domain().to_protocol_player(endpoint);
    }

    for p in db_players {
        if p.slot >= 4 {
            warn!(slot = p.slot, "Received invalid player slot");
        }
    }

    let guild_ids: [u32; 4] = [0; 4];
    let guild_names: [GuildName; MAX_PLAYER_SLOTS] = std::array::from_fn(|_| GuildName::default());

    Ok(SelectS2cSpecific::SetPlayerChoices {
        players,
        guild_ids,
        guild_names,
    }
    .into())
}

fn unroutable_endpoint() -> PlayerEndpoint {
    PlayerEndpoint {
        srv_ipv4_addr: 0,
        srv_port: 0,
    }
}

async fn resolve_player_endpoint(
    player: &zohar_db::PlayerRow,
    state: &SelectCtx<'_>,
) -> PhaseResult<PlayerEndpoint> {
    let fallback_empire = state
        .runtime
        .db
        .profiles()
        .find_by_username(&state.username)
        .await
        .ok()
        .flatten()
        .and_then(|profile| profile.empire)
        .unwrap_or(DomainEmpire::Red);
    let map_code = resolve_player_map_code(player, state, fallback_empire)?;
    let endpoint = state
        .runtime
        .map_resolver
        .resolve(state.runtime.channel_id, &map_code)
        .await
        .map_err(|err| {
            warn!(
                error = ?err,
                player_id = ?player.id,
                map_code = %map_code,
                "Map routing resolve failed"
            );
            anyhow::anyhow!(
                "map routing resolve failed for player_id={:?} map_code={}: {err}",
                player.id,
                map_code
            )
        })?;
    let ip = match endpoint.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .ok_or_else(|| anyhow::anyhow!("non-ipv4 endpoint for map routing"))?,
    };

    Ok(PlayerEndpoint {
        srv_ipv4_addr: i32::from_le_bytes(ip.octets()),
        srv_port: endpoint.port(),
    })
}

fn resolve_player_map_code(
    player: &zohar_db::PlayerRow,
    state: &SelectCtx<'_>,
    fallback_empire: DomainEmpire,
) -> anyhow::Result<String> {
    match &state.runtime.routing {
        SelectRouting::Core { coords } => {
            let spawn = coords.resolve_spawn_for_player(Some(player), fallback_empire);
            coords
                .map_code_by_id(spawn.map_id)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow::anyhow!("missing map code for map_id={}", spawn.map_id.get())
                })
        }
        SelectRouting::Gateway { empire_start_maps } => {
            // Gateway routing only needs map identity. Core remains source of truth for exact
            // spawn coordinates when DB position is missing.
            if let Some(map_key) = player.map_key.as_deref().filter(|value| !value.is_empty()) {
                Ok(map_key.to_owned())
            } else {
                Ok(empire_start_maps
                    .map_code_for_empire(fallback_empire)
                    .to_owned())
            }
        }
    }
}
