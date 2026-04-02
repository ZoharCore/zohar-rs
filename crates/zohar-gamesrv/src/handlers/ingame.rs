//! In-game phase handler.
//!
//! Main game loop - handles movement, combat, chat, etc.

use super::control::{ControlDecision, handle_session_control};
use super::runtime::{
    PhaseEffects, base_phase_span, disconnect, make_heartbeat_interval, run_phase,
};
use super::session_health::{SessionTick, SessionTracker};
use super::types::{PhaseResult, SessionEnd};
use crate::GameContext;
use crate::adapters::ToProtocol;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;
use zohar_db::{GameDb, PlayersView, ProfilesView, SessionsView};
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::{Empire as DomainEmpire, MapId};
use zohar_map_port::{EnterMsg, LeaveMsg, PlayerEvent};
use zohar_net::connection::NextConnection;
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::ingame::{InGameC2s, InGameS2c, system};
use zohar_protocol::handshake::HandshakeState;

pub(super) mod chat;
pub(super) mod combat;
pub(super) mod fishing;
pub(super) mod guild;
pub(super) mod movement;
pub(super) mod trading;
pub(super) mod world;

pub(super) type ThisPhase = zohar_net::connection::game_conn::InGame;

pub(super) struct InGameCtx<'a> {
    ctx: Arc<GameContext>,
    handshake: &'a mut HandshakeState,
    session: &'a mut SessionTracker,
    pub(super) username: String,
    pub(super) player_name: String,
    pub(super) player_id: zohar_domain::entity::player::PlayerId,
    pub(super) map_id: MapId,
    pub(super) player_empire: DomainEmpire,
}

const MAP_EVENT_BURST_LIMIT: usize = 32;

async fn handle_enter(state: &mut InGameCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let mut effects = PhaseEffects::empty();
    let channel_id = state.ctx.channel_id.min(u8::MAX as u32) as u8;

    effects.push(InGameS2c::System(system::SystemS2c::SetServerTime {
        time: state.handshake.uptime_at(Instant::now()).into(),
    }));
    effects.push(InGameS2c::System(system::SystemS2c::SetChannelInfo {
        channel_id,
    }));
    effects.push(
        ControlS2c::RequestHandshake {
            data: state.handshake.sync_data(Instant::now(), Duration::ZERO),
        }
        .into(),
    );
    Ok(effects)
}

async fn handle_tick(
    now: Instant,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
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
            let mut effects = PhaseEffects::empty();
            effects.push(ControlS2c::RequestHeartbeat.into());
            effects.push(InGameS2c::System(system::SystemS2c::SetServerTime {
                time: state.handshake.uptime_at(now).into(),
            }));
            Ok(effects)
        }
        Some(SessionTick::TimedOut) => Ok(PhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(PhaseEffects::empty()),
    }
}

async fn handle_packet(
    packet: InGameC2s,
    now: Instant,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    state.session.mark_rx(now);
    match packet {
        InGameC2s::Control(packet) => match handle_session_control(packet, now, state.handshake)? {
            ControlDecision::Handled(outcome) => {
                let mut effects = PhaseEffects::empty();
                effects.extend(outcome.send);
                Ok(effects)
            }
            ControlDecision::Reject(reason) => Ok(PhaseEffects::disconnect(reason)),
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
        PlayerEvent::EntityMove(event) => movement::encode_entity_move(event, map_id, coords),
        PlayerEvent::EntityDespawn { entity_id } => world::encode_entity_despawn(entity_id),
        PlayerEvent::Chat {
            channel,
            sender_entity_id,
            empire,
            message,
        } => chat::encode_chat_event(channel, sender_entity_id, empire, message),
    }
}

async fn apply_effects(
    conn: &mut Connection<ThisPhase>,
    effects: PhaseEffects<ThisPhase>,
) -> PhaseResult<Option<()>> {
    for packet in effects.send {
        send_outbound_packet(conn, packet).await?;
    }
    if let Some(reason) = effects.disconnect {
        return Err(disconnect(reason));
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
    map_rx: &mut tokio::sync::mpsc::Receiver<PlayerEvent>,
    max_events: usize,
    map_id: MapId,
    coords: &crate::ContentCoords,
) -> PhaseResult<()> {
    for _ in 0..max_events {
        let Ok(event) = map_rx.try_recv() else {
            break;
        };
        let packets = map_event_to_packets(event, map_id, coords);
        for packet in packets {
            send_outbound_packet(conn, packet).await?;
        }
    }
    Ok(())
}

async fn drive_ingame(
    mut conn: Connection<ThisPhase>,
    state: &mut InGameCtx<'_>,
    mut map_rx: tokio::sync::mpsc::Receiver<PlayerEvent>,
) -> PhaseResult<NextConnection<ThisPhase>> {
    // Enter phase
    let effects = handle_enter(state).await?;
    if let Some(data) = apply_effects(&mut conn, effects).await? {
        return Ok(conn.into_next_with_phase(data).await?);
    }

    let mut heartbeat = make_heartbeat_interval(state.ctx.heartbeat_interval);
    heartbeat.tick().await;

    loop {
        // Keep outbound map traffic progressing even when inbound client traffic is
        // continuously ready, preventing observer movement starvation.
        drain_outbound_burst(
            &mut conn,
            &mut map_rx,
            MAP_EVENT_BURST_LIMIT,
            state.map_id,
            state.ctx.coords.as_ref(),
        )
        .await?;

        let effects = tokio::select! {
            packet = conn.recv() => {
                let packet = packet?.ok_or_else(|| disconnect("connection closed"))?;
                let now = Instant::now();
                handle_packet(packet, now, state).await?
            }
            _ = heartbeat.tick() => {
                let now = Instant::now();
                handle_tick(now, state).await?
            },
            outbound = map_rx.recv() => {
                if let Some(event) = outbound {
                    let packets = map_event_to_packets(event, state.map_id, state.ctx.coords.as_ref());
                    for packet in packets {
                        send_outbound_packet(&mut conn, packet).await?;
                    }
                }
                continue;
            }
        };

        if let Some(data) = apply_effects(&mut conn, effects).await? {
            return Ok(conn.into_next_with_phase(data).await?);
        }
    }
}

pub(crate) async fn run_ingame(
    conn: Connection<ThisPhase>,
    ctx: &Arc<GameContext>,
    handshake: &mut HandshakeState,
    session: &mut SessionTracker,
) -> Result<NextConnection<ThisPhase>, SessionEnd> {
    // 1. Setup Identity variables
    let username = conn.username().to_string();
    let end_username = username.clone();
    let player_name = conn.player_name().to_string();
    let net_id = conn.net_id();
    let player_id = conn.player_id();

    // 2. Fetch player position from database
    let player = ctx.db.players().find_by_id(player_id).await.ok().flatten();

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
        // TODO: only fetch empire from DB for fallback when both map and coords are NULL in DB (or invalid / out of bounds)
        .expect("need empire for fallback spawn");

    let resolved_spawn = ctx
        .coords
        .resolve_spawn_for_player(player.as_ref(), player_empire);
    if resolved_spawn.used_fallback {
        warn!(
            username = %username,
            map_key = player.as_ref().and_then(|p| p.map_key.as_deref()),
            local_x = player.as_ref().and_then(|p| p.local_x),
            local_y = player.as_ref().and_then(|p| p.local_y),
            empire = ?player_empire,
            "Falling back to empire start spawn"
        );
    }
    let map_id = resolved_spawn.map_id;
    let initial_pos = resolved_spawn.local_pos;
    let _spawn_pos = ctx
        .coords
        .local_to_world(map_id, initial_pos)
        .expect("resolved local spawn position must map to world coordinates");

    let Some(map_code) = ctx.coords.map_code_by_id(map_id) else {
        return Err(SessionEnd::AfterLogin {
            username: end_username.clone(),
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
            username: end_username.clone(),
        });
    }

    // 6. Build PlayerAppearance with pre-computed protocol values
    let player_class = player
        .as_ref()
        .map(|p| p.class)
        .unwrap_or(zohar_domain::entity::player::PlayerClass::Warrior);
    let player_gender = player
        .as_ref()
        .map(|p| p.gender)
        .unwrap_or(zohar_domain::entity::player::PlayerGender::Male);

    let base_appearance = player
        .as_ref()
        .map(|p| p.appearance)
        .unwrap_or(zohar_domain::entity::player::PlayerBaseAppearance::VariantA);

    let appearance = PlayerAppearance {
        name: player_name.clone(),
        class: player_class,
        gender: player_gender,
        empire: player_empire,
        body_part: base_appearance.to_protocol() as u16,
        level: player.as_ref().map(|p| p.level as u32).unwrap_or(1),
        move_speed: 100,
        attack_speed: 100,
        guild_id: 0,
    };

    // 7. Enter the Map
    let map_rx = match ctx.map_events.enter_player(EnterMsg {
        player_id,
        player_net_id: zohar_domain::entity::EntityId(net_id.into()),
        initial_pos,
        appearance,
    }) {
        Ok(map_rx) => map_rx,
        Err(err) => {
            warn!(error = ?err, "Failed to register player with map runtime");
            let (_tx, map_rx) = tokio::sync::mpsc::channel(1);
            map_rx
        }
    };

    // 8. Prepare State
    let mut state = InGameCtx {
        ctx: Arc::clone(ctx),
        handshake,
        session,
        username,
        player_name,
        player_id,
        map_id,
        player_empire,
    };

    let span = base_phase_span::<ThisPhase>();
    span.record("player", &conn.player_name());

    // 9. Run the Phase Loop
    let result = run_phase(
        "Player disconnected from game",
        SessionEnd::AfterLogin {
            username: end_username,
        },
        span,
        drive_ingame(conn, &mut state, map_rx),
    )
    .await;

    // 10. Cleanup
    let _ = ctx.map_events.send_player_leave(LeaveMsg {
        player_id,
        player_net_id: zohar_domain::entity::EntityId(net_id.into()),
    });

    result
}

#[cfg(test)]
mod tests {
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
}
