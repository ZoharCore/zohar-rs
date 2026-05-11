use crate::handlers::types::PhaseResult;

use super::{InGameCtx, InGamePhaseEffects, commit_player_exit};
use tracing::warn;
use zohar_domain::PlayerExitKind;
use zohar_domain::coords::{LocalPos, WorldPos};
use zohar_map_port::{LeaveMsg, PortalDestination};
use zohar_protocol::game_pkt::ingame::chat::ChatS2c;
use zohar_protocol::game_pkt::{ChatKind, ZeroOpt};
use zohar_protocol::game_pkt::{WireServerAddr, ingame::system::SystemS2c};

#[derive(Debug)]
pub(super) struct ResolvedRelocation {
    map_id: zohar_domain::MapId,
    local_pos: LocalPos,
}

pub(super) fn resolve_world_relocation(
    state: &InGameCtx<'_>,
    world_pos: WorldPos,
) -> Result<ResolvedRelocation, RelocationError> {
    let Some((map_id, local_pos)) = state.ctx.coords.resolve_world_destination(world_pos) else {
        return Err(RelocationError::UnknownWorldDestination);
    };

    Ok(ResolvedRelocation { map_id, local_pos })
}

pub(super) fn resolve_local_relocation(
    state: &InGameCtx<'_>,
    local_pos: LocalPos,
) -> Result<ResolvedRelocation, RelocationError> {
    let Some(_) = state.ctx.coords.local_to_world(&state.map_id, local_pos) else {
        return Err(RelocationError::LocalDestinationOutOfBounds);
    };

    Ok(ResolvedRelocation {
        map_id: state.map_id.clone(),
        local_pos,
    })
}
pub(super) fn resolve_map_id_relocation(
    state: &InGameCtx<'_>,
    map_id: &zohar_domain::MapId,
) -> Result<ResolvedRelocation, RelocationError> {
    if !state.ctx.coords.is_valid_map(map_id) {
        return Err(RelocationError::UnknownMapCode);
    }

    let local_pos = state
        .ctx
        .coords
        .resolve_town_spawn(map_id, state.player_empire)
        .ok_or(RelocationError::AmbiguousTownSpawn)?;

    Ok(ResolvedRelocation {
        map_id: map_id.clone(),
        local_pos,
    })
}

#[allow(dead_code)]
fn resolve_relocation(
    _state: &InGameCtx<'_>,
    map_id: zohar_domain::MapId,
    local_pos: LocalPos,
) -> Result<ResolvedRelocation, RelocationError> {
    Ok(ResolvedRelocation { map_id, local_pos })
}

pub(super) async fn dispatch_handoff(
    state: &mut InGameCtx<'_>,
    source: &'static str,
    relocation: ResolvedRelocation,
) -> Result<InGamePhaseEffects, RelocationError> {
    let ResolvedRelocation { map_id, local_pos } = relocation;

    let endpoint = match state
        .ctx
        .map_resolver
        .resolve(state.ctx.channel_id, map_id.as_str())
        .await
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            warn!(
                source,
                player_id = ?state.player_id,
                map_id = %map_id,
                error = ?error,
                "Failed to resolve relocation destination endpoint"
            );
            return Err(RelocationError::RoutingUnavailable);
        }
    };

    let destination_addr =
        WireServerAddr::from_socket_addr(endpoint).ok_or(RelocationError::NonIpv4Endpoint)?;

    let snapshot = match state
        .ctx
        .map_events
        .capture_player_snapshot(LeaveMsg {
            player_id: state.player_id,
            player_net_id: state.player_net_id,
        })
        .await
    {
        Ok(snapshot) => snapshot.with_runtime_location(map_id.clone(), local_pos),
        Err(error) => {
            warn!(
                source,
                username = %state.username,
                player_id = ?state.player_id,
                map_id = %map_id,
                error = %error,
                "Failed to capture player snapshot before handoff"
            );
            return Err(RelocationError::CommitFailed);
        }
    };

    if let Err(error) = commit_player_exit(
        &state.ctx,
        PlayerExitKind::Handoff,
        &state.username,
        &state.connection_id,
        snapshot,
    )
    .await
    {
        warn!(
            source,
            username = %state.username,
            player_id = ?state.player_id,
            map_id = %map_id,
            error = %error,
            "Failed to prepare player handoff"
        );
        return Err(RelocationError::CommitFailed);
    }

    Ok(
        InGamePhaseEffects::send(SystemS2c::InitServerHandoff { destination_addr }.into())
            .with_handoff_disconnect("relocation handoff"),
    )
}

pub(super) async fn handle_portal_entry(
    state: &mut InGameCtx<'_>,
    destination: PortalDestination,
) -> PhaseResult<InGamePhaseEffects> {
    match destination {
        PortalDestination::MapTransfer { world_pos } => {
            match resolve_world_relocation(state, world_pos) {
                Ok(destination) => Ok(dispatch_handoff(state, "portal", destination)
                    .await
                    .unwrap_or_else(|error| {
                        warn!(
                            player_id = ?state.player_id,
                            world_x = world_pos.x,
                            world_y = world_pos.y,
                            error = %error,
                            "Failed to execute portal handoff"
                        );
                        portal_info_feedback(error.to_string())
                    })),
                Err(error) => {
                    warn!(
                        player_id = ?state.player_id,
                        world_x = world_pos.x,
                        world_y = world_pos.y,
                        error = %error,
                        "Portal target does not resolve to a known destination"
                    );
                    Ok(portal_info_feedback(error.to_string()))
                }
            }
        }
        PortalDestination::LocalReposition { local_pos } => {
            warn!(
                player_id = ?state.player_id,
                local_x = local_pos.x,
                local_y = local_pos.y,
                "Ignoring local-reposition portal until same-map relocation is implemented"
            );
            Ok(portal_info_feedback("Portal is not supported yet."))
        }
    }
}

pub(super) async fn handle_restart_town(
    state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    match resolve_town_relocation(state) {
        Ok(destination) => Ok(dispatch_handoff(state, "restart_town", destination)
            .await
            .unwrap_or_else(|error| {
                warn!(
                    player_id = ?state.player_id,
                    error = %error,
                    "Failed to execute restart-town handoff"
                );
                relocation_info_feedback("restart_town", error.to_string())
            })),
        Err(error) => {
            warn!(
                player_id = ?state.player_id,
                error = %error,
                "Restart-town target does not resolve to a known destination"
            );
            Ok(relocation_info_feedback("restart_town", error.to_string()))
        }
    }
}

fn resolve_town_relocation(state: &InGameCtx<'_>) -> Result<ResolvedRelocation, RelocationError> {
    let spawn = state
        .ctx
        .coords
        .resolve_town_restart(&state.map_id, state.player_empire);

    Ok(ResolvedRelocation {
        map_id: spawn.map_id,
        local_pos: spawn.local_pos,
    })
}

fn portal_info_feedback(message: impl AsRef<str>) -> InGamePhaseEffects {
    relocation_info_feedback("Portal", message)
}

fn relocation_info_feedback(prefix: &str, message: impl AsRef<str>) -> InGamePhaseEffects {
    InGamePhaseEffects::send(
        ChatS2c::NotifyChatMessage {
            kind: ChatKind::Info,
            message: format!("[{prefix}] {}\0", message.as_ref()).into_bytes(),
            net_id: ZeroOpt::none(),
            empire: ZeroOpt::none(),
        }
        .into(),
    )
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RelocationError {
    #[error("Target does not map to a known position in current content.")]
    UnknownWorldDestination,

    #[error("Target is outside the current map bounds.")]
    LocalDestinationOutOfBounds,

    #[error("Unknown map code.")]
    UnknownMapCode,

    #[error("Target does not have a unique town spawn for routing.")]
    AmbiguousTownSpawn,

    #[error("Target resolved to an invalid map position.")]
    #[allow(dead_code)]
    InvalidResolvedMapPosition,

    #[error("Map routing is unavailable for that destination.")]
    RoutingUnavailable,

    #[error("Map routing returned a non-IPv4 endpoint.")]
    NonIpv4Endpoint,

    #[error("Relocation failed. Please try again.")]
    CommitFailed,
}
