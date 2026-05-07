use super::{InGamePhaseEffects, try_send_client_intent};
use crate::handlers::ingame::InGameCtx;
use zohar_map_port::{ClientIntent, PlayerRestartIntent};

#[derive(clap::Subcommand, Debug, Clone, PartialEq)]
pub(in crate::handlers::ingame::chat) enum RestartCommand {
    #[command(name = "restart_here", about = "Restart at your current position.")]
    Here,

    #[command(name = "restart_town", about = "Restart at your empire town spawn.")]
    Town,
}

impl RestartCommand {
    pub(super) fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        let (intent, command_name) = match self {
            Self::Here => (PlayerRestartIntent::Here, "restart_here"),
            Self::Town => (PlayerRestartIntent::Town, "restart_town"),
        };

        try_send_client_intent(state, ClientIntent::Restart(intent), command_name)
    }
}
