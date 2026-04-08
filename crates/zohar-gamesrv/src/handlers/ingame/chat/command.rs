use super::super::InGameCtx;
use super::{ChatKind, ChatS2c, InGamePhaseEffects, ZeroOpt};
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use tracing::warn;
use zohar_map_port::{ClientIntent, ClientIntentMsg};

mod prefs;
mod session;

#[derive(Parser, Debug)]
#[command(
    name = "/",
    about = "In-game slash commands.",
    infer_subcommands = true,
    disable_colored_help = true,
    disable_version_flag = true
)]
struct SlashCommandLine {
    #[command(subcommand)]
    command: KnownCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(super) enum KnownCommand {
    #[command(flatten)]
    Session(session::SessionCommand),
    #[command(flatten)]
    Movement(prefs::PreferencesCommand),
}

impl KnownCommand {
    fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        match self {
            Self::Session(command) => command.execute(),
            Self::Movement(command) => command.execute(state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParsedCommand {
    Unknown { spelled: String },
    Feedback { message: String },
    Known(KnownCommand),
}

pub(super) fn parse(input: &str) -> Option<ParsedCommand> {
    let body = input.trim().strip_prefix('/')?;
    let tokens = tokenize(body);
    let spelled = format!(
        "/{}",
        tokens.first().map(String::as_str).unwrap_or_default()
    );

    if let [command, flag] = tokens.as_slice()
        && matches!(flag.as_str(), "--help" | "-h")
    {
        return match command_summary(command) {
            Some(message) => Some(ParsedCommand::Feedback { message }),
            None => Some(ParsedCommand::Unknown { spelled }),
        };
    }

    let argv = std::iter::once("chat-command").chain(tokens.iter().map(String::as_str));
    match SlashCommandLine::try_parse_from(argv) {
        Ok(parsed) => Some(ParsedCommand::Known(parsed.command)),
        Err(err) if err.kind() == ErrorKind::InvalidSubcommand => {
            Some(ParsedCommand::Unknown { spelled })
        }
        Err(err) => Some(ParsedCommand::Feedback {
            message: err
                .to_string()
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("Invalid command.")
                .to_string(),
        }),
    }
}

pub(super) fn execute(command: ParsedCommand, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
    match command {
        ParsedCommand::Known(command) => command.execute(state),
        ParsedCommand::Feedback { message } => InGamePhaseEffects::send(
            ChatS2c::NotifyChatMessage {
                kind: ChatKind::Info,
                message: format!("{message}\0").into_bytes(),
                net_id: ZeroOpt::none(),
                empire: ZeroOpt::none(),
            }
            .into(),
        ),
        ParsedCommand::Unknown { spelled } => InGamePhaseEffects::send(
            ChatS2c::NotifyChatMessage {
                kind: ChatKind::Info,
                message: format!("Unimplemented command: `{spelled}`.\0").into_bytes(),
                net_id: ZeroOpt::none(),
                empire: ZeroOpt::none(),
            }
            .into(),
        ),
    }
}

pub(super) fn try_send_client_intent(
    state: &mut InGameCtx<'_>,
    intent: ClientIntent,
    action_name: &'static str,
) -> InGamePhaseEffects {
    if let Err(err) = state
        .ctx
        .map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id: state.player_id,
            intent,
        })
    {
        warn!(
            player_id = ?state.player_id,
            map_id = state.map_id.get(),
            error = ?err,
            action = action_name,
            "Failed to enqueue client intent to map runtime"
        );

        InGamePhaseEffects::send(
            ChatS2c::NotifyChatMessage {
                kind: ChatKind::Info,
                message: b"Server is busy. Please try again.\0".to_vec(),
                net_id: ZeroOpt::none(),
                empire: ZeroOpt::none(),
            }
            .into(),
        )
    } else {
        InGamePhaseEffects::empty()
    }
}

fn command_summary(name: &str) -> Option<String> {
    let cmd = SlashCommandLine::command();
    let sub = cmd
        .get_subcommands()
        .find(|sub| sub.get_name() == name || sub.get_all_aliases().any(|alias| alias == name))?;
    let about = sub
        .get_about()
        .map(|styled| styled.to_string())
        .unwrap_or_else(|| "No help available.".to_string());
    Some(format!("/{name}: {about}"))
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.trim().chars().peekable();
    let mut quote = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(delim) if ch == delim => {
                quote = None;
            }
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
            }
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                while chars.next_if(|next| next.is_whitespace()).is_some() {}
            }
            None => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_protocol::game_pkt::InGameS2c;

    #[test]
    fn parser_returns_none_for_normal_chat() {
        assert_eq!(parse("hello world"), None);
    }

    #[test]
    fn parser_uses_clap_for_known_command_variants() {
        assert_eq!(
            parse("/logout"),
            Some(ParsedCommand::Known(KnownCommand::Session(
                session::SessionCommand::Logout,
            )))
        );
        assert_eq!(
            parse("/set_walk_mode"),
            Some(ParsedCommand::Known(KnownCommand::Movement(
                prefs::PreferencesCommand::SetWalkMode,
            )))
        );
    }

    #[test]
    fn parser_preserves_spelling_for_unknown_commands() {
        assert_eq!(
            parse("/FoObAr later"),
            Some(ParsedCommand::Unknown {
                spelled: "/FoObAr".to_string(),
            })
        );
    }

    #[test]
    fn tokenization_preserves_quoted_segments() {
        assert_eq!(
            tokenize(r#"block_chat "Alice Bob" 10m"#),
            vec![
                "block_chat".to_string(),
                "Alice Bob".to_string(),
                "10m".to_string()
            ]
        );
    }

    #[test]
    fn legacy_aliases_parse_as_known_commands() {
        assert_eq!(
            parse("/phase_selec"),
            Some(ParsedCommand::Known(KnownCommand::Session(
                session::SessionCommand::PhaseSelect,
            )))
        );
        assert_eq!(
            parse("/logou"),
            Some(ParsedCommand::Known(KnownCommand::Session(
                session::SessionCommand::Logout,
            )))
        );
        assert_eq!(
            parse("/qui"),
            Some(ParsedCommand::Known(KnownCommand::Session(
                session::SessionCommand::Quit,
            )))
        );
    }

    #[test]
    fn help_flag_returns_single_line_summary() {
        assert_eq!(
            parse("/logout --help"),
            Some(ParsedCommand::Feedback {
                message: "/logout: Disconnect back to the login screen.".to_string(),
            })
        );
        assert_eq!(
            parse("/set_walk_mode --help"),
            Some(ParsedCommand::Feedback {
                message: "/set_walk_mode: Switch your movement animation to walk.".to_string(),
            })
        );
    }

    #[test]
    fn invalid_usage_returns_single_line_error() {
        let Some(ParsedCommand::Feedback { message }) = parse("/logout extra") else {
            panic!("expected invalid-usage feedback");
        };

        assert!(message.contains("unexpected argument"));
    }

    #[test]
    fn unknown_commands_send_private_info_feedback() {
        let effects = execute(
            ParsedCommand::Unknown {
                spelled: "/foobar".to_string(),
            },
            panic_state(),
        );

        assert!(effects.transition.is_none());
        assert!(effects.disconnect.is_none());
        assert_eq!(effects.send.len(), 1);

        let InGameS2c::Chat(ChatS2c::NotifyChatMessage {
            kind,
            net_id,
            empire,
            message,
            ..
        }) = &effects.send[0]
        else {
            panic!("expected chat packet");
        };

        assert_eq!(*kind, ChatKind::Info);
        assert_eq!(*net_id, ZeroOpt::none());
        assert_eq!(*empire, ZeroOpt::none());
        assert_eq!(message, &b"Unimplemented command `/foobar`\0".to_vec());
    }

    fn panic_state() -> &'static mut InGameCtx<'static> {
        panic!("tests should not execute known commands")
    }
}
