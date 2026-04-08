use super::{ChatS2c, InGamePhaseEffects, ZeroOpt};
use zohar_protocol::game_pkt::ChatKind;

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::ingame::chat) enum SessionCommand {
    #[command(name = "phase_select", about = "Go to character select, same channel.")]
    PhaseSelect,
    #[command(name = "logout", about = "Disconnect back to the login screen.")]
    Logout,
    #[command(name = "quit", about = "Disconnect and exit the game client.")]
    Quit,
}

impl SessionCommand {
    pub(super) fn execute(self) -> InGamePhaseEffects {
        match self {
            Self::PhaseSelect => InGamePhaseEffects::transition(()),
            Self::Logout => InGamePhaseEffects::send(
                ChatS2c::NotifyChatMessage {
                    kind: ChatKind::Info,
                    message: b"Back to login window. Please wait.\0".to_vec(),
                    net_id: ZeroOpt::none(),
                    empire: ZeroOpt::none(),
                }
                .into(),
            )
            .with_disconnect("client requested logout"),
            Self::Quit => InGamePhaseEffects::send(
                ChatS2c::NotifyChatMessage {
                    kind: ChatKind::Command,
                    message: b"quit\0".to_vec(),
                    net_id: ZeroOpt::none(),
                    empire: ZeroOpt::none(),
                }
                .into(),
            )
            .with_disconnect("client requested quit"),
        }
    }
}
