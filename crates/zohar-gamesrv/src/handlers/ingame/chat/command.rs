use super::super::InGameCtx;
use super::{ChatKind, ChatS2c, InGamePhaseEffects, ZeroOpt};
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use tracing::warn;
use zohar_map_port::{ClientIntent, ClientIntentMsg};

mod prefs;
mod session;
mod stats;
mod teleport;

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

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub(super) enum KnownCommand {
    #[command(flatten)]
    Session(session::SessionCommand),
    #[command(flatten)]
    Preferences(prefs::PreferencesCommand),
    #[command(flatten)]
    Stats(stats::StatsCommand),
    #[command(flatten)]
    Teleport(teleport::TeleportCommand),
}

impl KnownCommand {
    async fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        match self {
            Self::Session(command) => command.execute(),
            Self::Preferences(command) => command.execute(state),
            Self::Stats(command) => command.execute(state),
            Self::Teleport(command) => command.execute(state).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ParsedCommand {
    Unknown { spelled: String, raw_full: String },
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
            None => Some(ParsedCommand::Unknown {
                spelled,
                raw_full: input.to_string(),
            }),
        };
    }

    let argv = std::iter::once("chat-command").chain(tokens.iter().map(String::as_str));
    match SlashCommandLine::try_parse_from(argv) {
        Ok(parsed) => Some(ParsedCommand::Known(parsed.command)),
        Err(err) if err.kind() == ErrorKind::InvalidSubcommand => Some(ParsedCommand::Unknown {
            spelled,
            raw_full: input.to_string(),
        }),
        Err(err) => Some(ParsedCommand::Feedback {
            message: format_clap_feedback(&spelled, &err),
        }),
    }
}

pub(super) async fn execute(
    command: ParsedCommand,
    state: &mut InGameCtx<'_>,
) -> InGamePhaseEffects {
    match command {
        ParsedCommand::Known(command) => command.execute(state).await,
        ParsedCommand::Feedback { message } => info_feedback(message),
        ParsedCommand::Unknown { spelled, raw_full } => {
            prefixed_info_feedback(&spelled, format!("Unimplemented command: `{raw_full}`."))
        }
    }
}

fn info_feedback(message: String) -> InGamePhaseEffects {
    InGamePhaseEffects::send(
        ChatS2c::NotifyChatMessage {
            kind: ChatKind::Info,
            message: format!("{message}\0").into_bytes(),
            net_id: ZeroOpt::none(),
            empire: ZeroOpt::none(),
        }
        .into(),
    )
}

pub(super) fn prefixed_info_feedback(
    command_name: &str,
    message: impl AsRef<str>,
) -> InGamePhaseEffects {
    let command_name = normalize_command_name(command_name);
    let message = collapse_to_one_line(message.as_ref());
    info_feedback(format!("{command_name}: {message}"))
}

pub(super) fn try_send_client_intent(
    state: &mut InGameCtx<'_>,
    intent: ClientIntent,
    command_name: &'static str,
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
            action = command_name,
            "Failed to enqueue client intent to map runtime"
        );

        prefixed_info_feedback(command_name, "Server is busy. Please try again.")
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
    let usage = normalize_usage(sub.clone().render_usage().to_string());
    Some(format!(
        "{}: {} Usage: {}.",
        normalize_command_name(name),
        collapse_to_one_line(&about),
        usage
    ))
}

fn format_clap_feedback(spelled: &str, err: &clap::Error) -> String {
    let mut details = Vec::new();
    let mut usage = None;
    let mut collecting_error_details = false;

    for line in err
        .to_string()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(rest) = line.strip_prefix("error:") {
            details.push(rest.trim().to_string());
            collecting_error_details = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix("Usage:") {
            usage = Some(normalize_usage(rest.trim()));
            collecting_error_details = false;
            continue;
        }

        if line.starts_with("For more information") {
            continue;
        }

        if collecting_error_details {
            details.push(line.to_string());
        }
    }

    let mut message = collapse_to_one_line(&details.join(" "));
    if message.is_empty() {
        message = "Invalid command.".to_string();
    }
    if let Some(usage) = usage {
        message.push_str(" Usage: ");
        message.push_str(&usage);
        message.push('.');
    }

    format!("{}: {}", normalize_command_name(spelled), message)
}

fn normalize_command_name(command_name: &str) -> String {
    if command_name.starts_with('/') {
        command_name.to_string()
    } else {
        format!("/{command_name}")
    }
}

fn normalize_usage(usage: impl AsRef<str>) -> String {
    let usage = usage.as_ref().trim();
    let usage = usage.strip_prefix("Usage:").unwrap_or(usage).trim();
    let usage = usage.strip_prefix("chat-command ").unwrap_or(usage);
    let usage = usage.strip_prefix('/').unwrap_or(usage);
    format!("/{usage}")
}

fn collapse_to_one_line(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
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
            Some(ParsedCommand::Known(KnownCommand::Preferences(
                prefs::PreferencesCommand::SetWalkMode,
            )))
        );
        assert_eq!(
            parse("/warp 512 768"),
            Some(ParsedCommand::Known(KnownCommand::Teleport(
                teleport::TeleportCommand::Warp { x: 512.0, y: 768.0 },
            )))
        );
        assert_eq!(
            parse("/goto a1"),
            Some(ParsedCommand::Known(KnownCommand::Teleport(
                teleport::TeleportCommand::Goto {
                    target: "a1".to_string(),
                    y: None,
                },
            )))
        );
        assert_eq!(
            parse("/goto 512 768"),
            Some(ParsedCommand::Known(KnownCommand::Teleport(
                teleport::TeleportCommand::Goto {
                    target: "512".to_string(),
                    y: Some("768".to_string()),
                },
            )))
        );
    }

    #[test]
    fn parser_preserves_spelling_for_unknown_commands() {
        assert_eq!(
            parse("/FoObAr later"),
            Some(ParsedCommand::Unknown {
                spelled: "/FoObAr".to_string(),
                raw_full: "/FoObAr later".to_string(),
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
        let Some(ParsedCommand::Feedback { message }) = parse("/logout --help") else {
            panic!("expected help feedback");
        };
        assert!(message.starts_with("/logout:"));
        assert!(message.contains("Disconnect back to the login screen."));
        assert!(message.contains("Usage: /logout."));
        assert!(!message.contains('\n'));

        let Some(ParsedCommand::Feedback { message }) = parse("/set_walk_mode --help") else {
            panic!("expected help feedback");
        };
        assert!(message.starts_with("/set_walk_mode:"));
        assert!(message.contains("Set movement animation to walking."));
        assert!(message.contains("Usage: /set_walk_mode."));
        assert!(!message.contains('\n'));

        let Some(ParsedCommand::Feedback { message }) = parse("/goto --help") else {
            panic!("expected help feedback");
        };
        assert!(message.starts_with("/goto:"));
        assert!(message.contains("Usage: /goto <MAP_CODE_OR_LOCAL_X_M> [LOCAL_Y_M]."));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn invalid_usage_returns_single_line_error() {
        let Some(ParsedCommand::Feedback { message }) = parse("/logout extra") else {
            panic!("expected invalid-usage feedback");
        };

        assert!(message.starts_with("/logout:"));
        assert!(message.contains("unexpected argument"));
        assert!(message.contains("Usage: /logout."));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn missing_args_include_usage_on_one_line() {
        let Some(ParsedCommand::Feedback { message }) = parse("/goto") else {
            panic!("expected invalid-usage feedback");
        };

        assert!(message.starts_with("/goto:"));
        assert!(message.contains("required arguments were not provided"));
        assert!(message.contains("Usage: /goto <MAP_CODE_OR_LOCAL_X_M> [LOCAL_Y_M]."));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn unknown_commands_send_private_info_feedback() {
        let effects = prefixed_info_feedback("/foobar", "Unimplemented command: `/foobar`.");

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
        assert_eq!(
            message,
            &b"/foobar: Unimplemented command: `/foobar`.\0".to_vec()
        );
    }
}
