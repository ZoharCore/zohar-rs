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
    map_code: String,
    local_pos: LocalPos,
}

pub(super) fn resolve_world_relocation(
    state: &InGameCtx<'_>,
    world_pos: WorldPos,
) -> Result<ResolvedRelocation, RelocationError> {
    let Some((map_id, local_pos)) = state.ctx.coords.resolve_world_destination(world_pos) else {
        return Err(RelocationError::UnknownWorldDestination);
    };

    resolve_relocation(state, map_id, local_pos)
}

pub(super) fn resolve_local_relocation(
    state: &InGameCtx<'_>,
    local_pos: LocalPos,
) -> Result<ResolvedRelocation, RelocationError> {
    let Some(_) = state.ctx.coords.local_to_world(state.map_id, local_pos) else {
        return Err(RelocationError::LocalDestinationOutOfBounds);
    };

    resolve_relocation(state, state.map_id, local_pos)
}

pub(super) fn resolve_map_code_relocation(
    state: &InGameCtx<'_>,
    map_code: &str,
) -> Result<ResolvedRelocation, RelocationError> {
    let map_id = state
        .ctx
        .coords
        .map_id_by_code(map_code)
        .ok_or(RelocationError::UnknownMapCode)?;

    let local_pos = state
        .ctx
        .coords
        .resolve_town_spawn(map_id, state.player_empire)
        .ok_or(RelocationError::AmbiguousTownSpawn)?;

    let Some(_) = state.ctx.coords.local_to_world(map_id, local_pos) else {
        return Err(RelocationError::InvalidResolvedMapPosition);
    };

    resolve_relocation(state, map_id, local_pos)
}

fn resolve_relocation(
    state: &InGameCtx<'_>,
    map_id: zohar_domain::MapId,
    _local_pos: LocalPos,
) -> Result<ResolvedRelocation, RelocationError> {
    let map_code = state
        .ctx
        .coords
        .map_code_by_id(map_id)
        .map(ToOwned::to_owned)
        .ok_or(RelocationError::RoutingUnavailable)?;

    Ok(ResolvedRelocation {
        map_id,
        map_code,
        local_pos: _local_pos,
    })
}

pub(super) async fn dispatch_handoff(
    state: &mut InGameCtx<'_>,
    source: &'static str,
    relocation: ResolvedRelocation,
) -> Result<InGamePhaseEffects, RelocationError> {
    let ResolvedRelocation {
        map_id,
        map_code,
        local_pos,
    } = relocation;

    let endpoint = match state
        .ctx
        .map_resolver
        .resolve(state.ctx.channel_id, &map_code)
        .await
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            warn!(
                source,
                player_id = ?state.player_id,
                map_id = map_id.get(),
                map_code = %map_code,
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
        Ok(snapshot) => snapshot.with_runtime_location(map_code.clone(), local_pos),
        Err(error) => {
            warn!(
                source,
                username = %state.username,
                player_id = ?state.player_id,
                map_id = map_id.get(),
                map_code = %map_code,
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
            map_id = map_id.get(),
            map_code = %map_code,
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

fn portal_info_feedback(message: impl AsRef<str>) -> InGamePhaseEffects {
    InGamePhaseEffects::send(
        ChatS2c::NotifyChatMessage {
            kind: ChatKind::Info,
            message: format!("[Portal] {}\0", message.as_ref()).into_bytes(),
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
    InvalidResolvedMapPosition,

    #[error("Map routing is unavailable for that destination.")]
    RoutingUnavailable,

    #[error("Map routing returned a non-IPv4 endpoint.")]
    NonIpv4Endpoint,

    #[error("Relocation failed. Please try again.")]
    CommitFailed,
}
