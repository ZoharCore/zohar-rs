use super::{InGamePhaseEffects, try_send_client_intent};
use crate::handlers::ingame::InGameCtx;
use zohar_domain::entity::MovementAnimation;
use zohar_map_port::ClientIntent;

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::ingame::chat) enum PreferencesCommand {
    #[command(name = "set_walk_mode", about = "Set movement animation to walking.")]
    SetWalkMode,
    #[command(name = "set_run_mode", about = "Set movement animation to running.")]
    SetRunMode,
}

impl PreferencesCommand {
    pub(super) fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        let animation = match self {
            Self::SetWalkMode => MovementAnimation::Walk,
            Self::SetRunMode => MovementAnimation::Run,
        };
        try_send_client_intent(
            state,
            ClientIntent::SetMovementAnimation(animation),
            "set movement animation",
        )
    }
}
