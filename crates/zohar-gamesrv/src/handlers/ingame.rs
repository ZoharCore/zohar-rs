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
use crate::adapters::{ToDomain, ToProtocol};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use zohar_db::{GameDb, PlayersView, ProfilesView, SessionsView};
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::{LocalPos, WorldPos};
use zohar_domain::{Empire as DomainEmpire, MapId};
use zohar_net::connection::NextConnection;
use zohar_net::{Connection, ConnectionPhaseExt};
use zohar_protocol::game_pkt::ControlS2c;
use zohar_protocol::game_pkt::ingame::world::WorldS2c;
use zohar_protocol::game_pkt::ingame::{InGameC2s, InGameS2c, chat as pkt_chat, movement, system};
use zohar_protocol::game_pkt::{ChatKind, NetId};
use zohar_protocol::handshake::HandshakeState;
use zohar_sim::{ClientIntentMsg, EnterMsg, LeaveMsg, LocalMapInbound, PlayerEvent, PlayerOutbox};

pub(super) mod chat;
pub(super) mod spawn;

pub(super) type ThisPhase = zohar_net::connection::game_conn::InGame;

pub(super) struct InGameCtx<'a> {
    ctx: Arc<GameContext>,
    handshake: &'a mut HandshakeState,
    session: &'a mut SessionTracker,
    pub(super) username: String,
    pub(super) player_name: String,
    pub(super) net_id: NetId,
    pub(super) player_id: zohar_domain::entity::player::PlayerId,
    pub(super) map_id: MapId,
    // Player data from DB
    pub(super) player_class: zohar_domain::entity::player::PlayerClass,
    pub(super) player_gender: zohar_domain::entity::player::PlayerGender,
    pub(super) base_appearance: zohar_domain::entity::player::PlayerBaseAppearance,
    pub(super) player_empire: DomainEmpire,
    pub(super) spawn_pos: WorldPos,
    pub(super) player_level: i32,
}

const MAP_EVENT_BURST_LIMIT: usize = 32;
const MOVEMENT_TS_SKEW_WARN_THRESHOLD_MS: u32 = 5_000;

async fn handle_enter(state: &mut InGameCtx<'_>) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let mut effects = spawn::enter_world_effects(state);
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
    let periodic_sync = state.handshake.sync_data(now, Duration::ZERO);
    let periodic_uptime_ms = u32::from(periodic_sync.time);
    debug!(
        username = %state.username,
        player_id = ?state.player_id,
        uptime_ms = periodic_uptime_ms,
        "Sending periodic in-game handshake resync"
    );

    let mut effects = PhaseEffects::empty();
    effects.push(
        ControlS2c::RequestHandshake {
            data: periodic_sync,
        }
        .into(),
    );

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
            effects.push(ControlS2c::RequestHeartbeat.into());
            effects.push(InGameS2c::System(system::SystemS2c::SetServerTime {
                time: state.handshake.uptime_at(now).into(),
            }));
            Ok(effects)
        }
        Some(SessionTick::TimedOut) => Ok(PhaseEffects::disconnect("heartbeat timeout")),
        None => Ok(effects),
    }
}

async fn handle_packet(
    packet: InGameC2s,
    now: Instant,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    state.session.mark_rx(now);
    match packet {
        InGameC2s::Control(control) => {
            match handle_session_control(control, now, state.handshake)? {
                ControlDecision::Handled(outcome) => {
                    let mut effects = PhaseEffects::empty();
                    effects.extend(outcome.send);
                    Ok(effects)
                }
                ControlDecision::Reject(reason) => Ok(PhaseEffects::disconnect(reason)),
            }
        }
        InGameC2s::Chat(pkt_chat::ChatC2s::SubmitChatMessage { kind, message, .. }) => {
            chat::handle_chat_message(kind, message, state).await
        }
        InGameC2s::Move(movement::MovementC2s::InputMovement {
            kind,
            arg,
            rot,
            x,
            y,
            ts,
        }) => {
            let kind = kind.to_domain();
            let packet_ts = u32::from(ts);
            let server_uptime_ms = state
                .handshake
                .uptime_at(now)
                .as_millis()
                .min(u128::from(u32::MAX)) as u32;
            let skew_ms = packet_ts.abs_diff(server_uptime_ms);
            if skew_ms > MOVEMENT_TS_SKEW_WARN_THRESHOLD_MS {
                warn!(
                    username = %state.username,
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    packet_ts,
                    server_uptime_ms,
                    skew_ms,
                    "Movement timestamp skew exceeds diagnostic threshold"
                );
            }

            // Send movement intent to MapActor for broadcast to all players
            let Some(local_pos) = state.ctx.coords.world_wire_to_local(state.map_id, x, y) else {
                warn!(
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    wire_x = i32::from(x),
                    wire_y = i32::from(y),
                    "Ignoring out-of-bounds movement position"
                );
                return Ok(PhaseEffects::empty());
            };
            let intent_msg = ClientIntentMsg {
                player_id: state.player_id,
                intent: zohar_sim::ClientIntent::Move {
                    entity_id: zohar_domain::entity::EntityId(state.net_id.into()),
                    kind,
                    arg,
                    rot,
                    x: local_pos.x,
                    y: local_pos.y,
                    // Preserve client-provided movement time (reference server behavior).
                    ts: packet_ts,
                },
            };
            if let Err(err) = state
                .ctx
                .map_events
                .try_send(LocalMapInbound::ClientIntent { msg: intent_msg })
            {
                warn!(
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    kind = ?kind,
                    ts = packet_ts,
                    error = ?err,
                    "Failed to enqueue movement intent to map runtime"
                );
            }
            Ok(PhaseEffects::empty())
        }
        InGameC2s::Trading(_) => {
            warn!("Unhandled in-game trading packet");
            Ok(PhaseEffects::disconnect("unhandled in-game trading packet"))
        }
        InGameC2s::Guild(_) => {
            warn!("Unhandled in-game guild packet");
            Ok(PhaseEffects::disconnect("unhandled in-game guild packet"))
        }
        InGameC2s::Fishing(_) => {
            warn!("Unhandled in-game fishing packet");
            Ok(PhaseEffects::disconnect("unhandled in-game fishing packet"))
        }
    }
}

fn map_event_to_packets(
    event: PlayerEvent,
    map_id: MapId,
    coords: &crate::ContentCoords,
) -> Vec<InGameS2c> {
    match event {
        PlayerEvent::EntitySpawn { show, details } => {
            let Some(world_pos) = coords.local_to_world(map_id, show.pos) else {
                return Vec::new();
            };
            let net_id = show.entity_id.to_protocol();

            // Derive entity_type and race_num from EntityKind variant
            let (entity_type, race_num) = show.kind.to_protocol();

            // Convert meter float world coords into centimeter i32 world coords
            let (x, y) = world_pos.to_protocol();

            let show_pkt = InGameS2c::World(WorldS2c::SpawnEntity {
                net_id,
                angle: show.angle,
                x,
                y,
                entity_type,
                race_num,
                move_speed: show.move_speed,
                attack_speed: show.attack_speed,
                state_flags: show.state_flags,
                buff_flags: show.buff_flags,
            });

            let mut out = vec![show_pkt];

            if let Some(details) = details {
                let details_pkt = InGameS2c::World(WorldS2c::SetEntityDetails {
                    net_id,
                    name: details.name.into(),
                    body_part: details.body_part,
                    wep_part: details.wep_part,
                    _reserved_part: 0,
                    hair_part: details.hair_part,
                    empire: details.empire.to_protocol(),
                    guild_id: details.guild_id,
                    level: details.level,
                    rank_pts: details.rank_pts,
                    pvp_mode: details.pvp_mode,
                    mount_id: details.mount_id,
                });

                out.push(details_pkt);
            }
            out
        }
        PlayerEvent::EntityMove {
            entity_id,
            kind,
            arg,
            rot,
            x,
            y,
            ts: source_ts,
            duration,
        } => {
            let local_pos = LocalPos::new(x, y);
            let Some(world_pos) = coords.local_to_world(map_id, local_pos) else {
                warn!(
                    map_id = map_id.get(),
                    entity_id = entity_id.0,
                    ?kind,
                    local_x = x,
                    local_y = y,
                    "Dropping movement packet due to out-of-bounds local position"
                );
                return Vec::new();
            };
            // Convert meter float world coords into centimeter i32 world coords
            let (x, y) = world_pos.to_protocol();

            let out = vec![InGameS2c::Move(movement::MovementS2c::SyncEntityMovement {
                kind: kind.to_protocol(),
                arg,
                rot,
                net_id: entity_id.to_protocol(),
                x,
                y,
                // Preserve source timestamp to match reference movement semantics.
                ts: source_ts.into(),
                duration: duration.into(),
            })];
            out
        }
        PlayerEvent::EntityDespawn { entity_id } => {
            vec![InGameS2c::World(WorldS2c::DestroyEntity {
                net_id: entity_id.to_protocol(),
            })]
        }
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            empire,
            message,
        } => {
            // TODO fix this monstrosity with proper num_enum or binrw repr u8 mappings
            let chat_kind = match kind {
                0 => ChatKind::Speak,
                1 => ChatKind::Info,
                2 => ChatKind::Notice,
                5 => ChatKind::Command,
                6 => ChatKind::Shout,
                _ => ChatKind::Info,
            };
            vec![InGameS2c::Chat(pkt_chat::ChatS2c::NotifyChatMessage {
                kind: chat_kind,
                net_id: zohar_protocol::game_pkt::ZeroOpt::from(
                    sender_entity_id.map(|id| id.to_protocol()),
                ),
                empire: zohar_protocol::game_pkt::ZeroOpt::from(empire.map(|e| e.to_protocol())),
                message,
            })]
        }
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
    let spawn_pos = ctx
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

    // 4. Create the Channel and Outbox for Map -> Player communication
    let (map_tx, map_rx) = tokio::sync::mpsc::channel(256);
    let outbox = PlayerOutbox::new(map_tx);

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
    if let Err(err) = ctx.map_events.send(LocalMapInbound::PlayerEnter {
        msg: EnterMsg {
            player_id,
            player_net_id: zohar_domain::entity::EntityId(net_id.into()),
            initial_pos,
            appearance,
            outbox,
        },
    }) {
        warn!(error = ?err, "Failed to register player with map runtime");
    }

    // 8. Prepare State
    let mut state = InGameCtx {
        ctx: Arc::clone(ctx),
        handshake,
        session,
        username,
        player_name,
        net_id,
        player_id,
        map_id,
        player_class,
        player_gender,
        base_appearance,
        player_empire,
        spawn_pos,
        player_level: player.as_ref().map(|p| p.level).unwrap_or(1),
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
    let _ = ctx.map_events.send(LocalMapInbound::PlayerLeave {
        msg: LeaveMsg {
            player_id,
            player_net_id: zohar_domain::entity::EntityId(net_id.into()),
        },
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
