use super::{InGamePhaseEffects, prefixed_info_feedback};
use crate::handlers::ingame::{InGameCtx, commit_player_exit};
use tracing::warn;
use zohar_domain::PlayerExitKind;
use zohar_domain::coords::{LocalPos, WorldPos};
use zohar_domain::entity::player::PlayerRuntimeSnapshot;
use zohar_protocol::game_pkt::{WireServerAddr, ingame::system::SystemS2c};

#[derive(clap::Subcommand, Debug, Clone, PartialEq)]
pub(in crate::handlers::ingame::chat) enum TeleportCommand {
    #[command(
        name = "warp",
        about = "Reconnect to the core owning an absolute world-space position in meters."
    )]
    Warp {
        #[arg(value_name = "WORLD_X_M")]
        x: f32,
        #[arg(value_name = "WORLD_Y_M")]
        y: f32,
    },

    #[command(
        name = "goto",
        about = "Reconnect to `/goto <LOCAL_X_M> <LOCAL_Y_M>` on the current map or `/goto <MAP_CODE>` to that map's town spawn."
    )]
    Goto {
        #[arg(value_name = "MAP_CODE_OR_LOCAL_X_M")]
        target: String,
        #[arg(value_name = "LOCAL_Y_M")]
        y: Option<String>,
    },
}

impl TeleportCommand {
    pub(super) async fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        match self {
            Self::Warp { x, y } => finish_cmd("warp", execute_warp(state, x, y).await),
            Self::Goto { target, y } => finish_cmd("goto", execute_goto(state, target, y).await),
        }
    }
}

fn finish_cmd(
    command_name: &'static str,
    result: Result<InGamePhaseEffects, TeleportError>,
) -> InGamePhaseEffects {
    match result {
        Ok(effects) => effects,
        Err(error) => prefixed_info_feedback(command_name, error.to_string()),
    }
}

async fn execute_warp(
    state: &mut InGameCtx<'_>,
    x: f32,
    y: f32,
) -> Result<InGamePhaseEffects, TeleportError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(TeleportError::NonFiniteWarpCoords);
    }

    let world_pos = WorldPos::new(x, y);
    let Some((map_id, local_pos)) = state.ctx.coords.resolve_world_destination(world_pos) else {
        return Err(TeleportError::UnknownWorldDestination);
    };

    let destination = resolve_destination(state, map_id, local_pos)?;
    dispatch_transfer(state, "warp", destination).await
}

async fn execute_goto(
    state: &mut InGameCtx<'_>,
    target: String,
    y: Option<String>,
) -> Result<InGamePhaseEffects, TeleportError> {
    let target = parse_goto_target(target, y)?;
    let destination = resolve_goto_destination(state, target)?;
    dispatch_transfer(state, "goto", destination).await
}

#[derive(Debug, Clone, PartialEq)]
enum GotoTarget {
    LocalCoords { x: f32, y: f32 },
    MapCode(String),
}

#[derive(Debug)]
struct ResolvedTeleport {
    map_id: zohar_domain::MapId,
    map_code: String,
    local_pos: LocalPos,
}

fn parse_goto_target(target: String, y: Option<String>) -> Result<GotoTarget, TeleportError> {
    match y {
        Some(y) => {
            let x = target
                .parse::<f32>()
                .map_err(|_| TeleportError::InvalidGotoUsage)?;
            let y = y
                .parse::<f32>()
                .map_err(|_| TeleportError::InvalidGotoUsage)?;
            Ok(GotoTarget::LocalCoords { x, y })
        }
        None => Ok(GotoTarget::MapCode(target)),
    }
}

fn resolve_goto_destination(
    state: &InGameCtx<'_>,
    target: GotoTarget,
) -> Result<ResolvedTeleport, TeleportError> {
    match target {
        GotoTarget::LocalCoords { x, y } => resolve_local_goto_destination(state, x, y),
        GotoTarget::MapCode(map_code) => resolve_map_code_destination(state, &map_code),
    }
}

fn resolve_local_goto_destination(
    state: &InGameCtx<'_>,
    x: f32,
    y: f32,
) -> Result<ResolvedTeleport, TeleportError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(TeleportError::NonFiniteGotoCoords);
    }

    let local_pos = LocalPos::new(x, y);
    let Some(_) = state.ctx.coords.local_to_world(state.map_id, local_pos) else {
        return Err(TeleportError::LocalDestinationOutOfBounds);
    };

    resolve_destination(state, state.map_id, local_pos)
}

fn resolve_map_code_destination(
    state: &InGameCtx<'_>,
    map_code: &str,
) -> Result<ResolvedTeleport, TeleportError> {
    let map_id = state
        .ctx
        .coords
        .map_id_by_code(map_code)
        .ok_or(TeleportError::UnknownMapCode)?;

    let local_pos = state
        .ctx
        .coords
        .resolve_town_spawn(map_id, state.player_empire)
        .ok_or(TeleportError::AmbiguousTownSpawn)?;

    let Some(_) = state.ctx.coords.local_to_world(map_id, local_pos) else {
        return Err(TeleportError::InvalidResolvedMapPosition);
    };

    resolve_destination(state, map_id, local_pos)
}

fn resolve_destination(
    state: &InGameCtx<'_>,
    map_id: zohar_domain::MapId,
    local_pos: LocalPos,
) -> Result<ResolvedTeleport, TeleportError> {
    let map_code = state
        .ctx
        .coords
        .map_code_by_id(map_id)
        .map(ToOwned::to_owned)
        .ok_or(TeleportError::RoutingUnavailable)?;

    Ok(ResolvedTeleport {
        map_id,
        map_code,
        local_pos,
    })
}

async fn dispatch_transfer(
    state: &mut InGameCtx<'_>,
    command_name: &'static str,
    destination: ResolvedTeleport,
) -> Result<InGamePhaseEffects, TeleportError> {
    let ResolvedTeleport {
        map_id,
        map_code,
        local_pos,
    } = destination;

    let endpoint = match state
        .ctx
        .map_resolver
        .resolve(state.ctx.channel_id, &map_code)
        .await
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            warn!(
                command = command_name,
                player_id = ?state.player_id,
                map_id = map_id.get(),
                map_code = %map_code,
                error = ?error,
                "Failed to resolve teleport destination endpoint"
            );
            return Err(TeleportError::RoutingUnavailable);
        }
    };

    let destination_addr =
        WireServerAddr::from_socket_addr(endpoint).ok_or(TeleportError::NonIpv4Endpoint)?;

    let snapshot = PlayerRuntimeSnapshot {
        id: state.player_id,
        runtime_epoch: state.player_runtime_epoch,
        map_key: map_code.clone(),
        local_pos,
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
            command = command_name,
            username = %state.username,
            player_id = ?state.player_id,
            map_id = map_id.get(),
            map_code = %map_code,
            error = %error,
            "Failed to prepare player handoff"
        );
        return Err(TeleportError::CommitFailed);
    }

    Ok(
        InGamePhaseEffects::send(SystemS2c::InitServerHandoff { destination_addr }.into())
            .with_handoff_disconnect("teleport handoff"),
    )
}

#[derive(Debug, thiserror::Error)]
enum TeleportError {
    #[error("Expected `/goto <LOCAL_X_M> <LOCAL_Y_M>` or `/goto <MAP_CODE>`.")]
    InvalidGotoUsage,

    #[error("Coordinates must be finite numbers.")]
    NonFiniteWarpCoords,

    #[error("Goto coordinates must be finite numbers.")]
    NonFiniteGotoCoords,

    #[error("Target does not map to a known position in current content.")]
    UnknownWorldDestination,

    #[error("Goto target is outside the current map bounds.")]
    LocalDestinationOutOfBounds,

    #[error("Unknown map code.")]
    UnknownMapCode,

    #[error("Goto target does not have a unique town spawn for routing.")]
    AmbiguousTownSpawn,

    #[error("Goto target resolved to an invalid map position.")]
    InvalidResolvedMapPosition,

    #[error("Map routing is unavailable for that destination.")]
    RoutingUnavailable,

    #[error("Map routing returned a non-IPv4 endpoint.")]
    NonIpv4Endpoint,

    #[error("Warp failed. Please try again.")]
    CommitFailed,
}
