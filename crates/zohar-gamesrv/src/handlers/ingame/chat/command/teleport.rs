use super::{InGamePhaseEffects, prefixed_info_feedback};
use crate::handlers::ingame::InGameCtx;
use crate::handlers::ingame::relocation::{
    RelocationError, ResolvedRelocation, dispatch_handoff, resolve_local_relocation,
    resolve_map_code_relocation, resolve_world_relocation,
};
use zohar_domain::coords::{LocalPos, WorldPos};

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
    result: Result<InGamePhaseEffects, TeleportCommandError>,
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
) -> Result<InGamePhaseEffects, TeleportCommandError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(TeleportCommandError::NonFiniteCoords);
    }

    let world_pos = WorldPos::new(x, y);
    let relocation =
        resolve_world_relocation(state, world_pos).map_err(TeleportCommandError::from)?;

    dispatch_handoff(state, "warp", relocation)
        .await
        .map_err(TeleportCommandError::from)
}

async fn execute_goto(
    state: &mut InGameCtx<'_>,
    target: String,
    y: Option<String>,
) -> Result<InGamePhaseEffects, TeleportCommandError> {
    let target = parse_goto_target(target, y)?;
    let relocation = resolve_goto_relocation(state, target)?;
    dispatch_handoff(state, "goto", relocation)
        .await
        .map_err(TeleportCommandError::from)
}

#[derive(Debug, Clone, PartialEq)]
enum GotoTarget {
    LocalCoords { x: f32, y: f32 },
    MapCode(String),
}

fn parse_goto_target(
    target: String,
    y: Option<String>,
) -> Result<GotoTarget, TeleportCommandError> {
    match y {
        Some(y) => {
            let x = target
                .parse::<f32>()
                .map_err(|_| TeleportCommandError::InvalidGotoUsage)?;
            let y = y
                .parse::<f32>()
                .map_err(|_| TeleportCommandError::InvalidGotoUsage)?;

            Ok(GotoTarget::LocalCoords { x, y })
        }
        None => Ok(GotoTarget::MapCode(target)),
    }
}

fn resolve_goto_relocation(
    state: &InGameCtx<'_>,
    target: GotoTarget,
) -> Result<ResolvedRelocation, TeleportCommandError> {
    match target {
        GotoTarget::LocalCoords { x, y } => resolve_local_goto_destination(state, x, y),
        GotoTarget::MapCode(map_code) => {
            resolve_map_code_relocation(state, &map_code).map_err(TeleportCommandError::from)
        }
    }
}

fn resolve_local_goto_destination(
    state: &InGameCtx<'_>,
    x: f32,
    y: f32,
) -> Result<ResolvedRelocation, TeleportCommandError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(TeleportCommandError::NonFiniteCoords);
    }

    resolve_local_relocation(state, LocalPos::new(x, y)).map_err(TeleportCommandError::from)
}

#[derive(Debug, thiserror::Error)]
enum TeleportCommandError {
    #[error("Expected `/goto <LOCAL_X_M> <LOCAL_Y_M>` or `/goto <MAP_CODE>`.")]
    InvalidGotoUsage,

    #[error("Coordinates must be finite numbers.")]
    NonFiniteCoords,

    #[error(transparent)]
    Relocation(#[from] RelocationError),
}
