use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use crate::infra::{ClusterEvent, GlobalShoutEvent};
use std::sync::Arc;
use tracing::{info, warn};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::chat;
use zohar_protocol::game_pkt::{ChatKind, ZeroOpt};
use zohar_sim::{ClientIntent, ClientIntentMsg, LocalMapInbound};

pub(super) async fn handle_chat_message(
    kind: ChatKind,
    message: Vec<u8>,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    let text = decode_cstr(&message);
    let cmd = text.trim().to_owned();
    match cmd.as_str() {
        "/phase_select" => {
            info!(kind = ?kind, "Returning to character select");
            Ok(PhaseEffects::transition(()))
        }
        "/logout" => {
            let mut effects =
                PhaseEffects::send(InGameS2c::Chat(chat::ChatS2c::NotifyChatMessage {
                    kind: ChatKind::Info,
                    net_id: ZeroOpt::none(),
                    empire: ZeroOpt::none(),
                    message: b"Back to login window. Please wait.\0".to_vec(),
                }));
            effects.disconnect = Some("client requested logout");
            Ok(effects)
        }
        "/quit" => {
            let mut effects =
                PhaseEffects::send(InGameS2c::Chat(chat::ChatS2c::NotifyChatMessage {
                    kind: ChatKind::Command,
                    net_id: ZeroOpt::none(),
                    empire: ZeroOpt::none(),
                    message: b"quit\0".to_vec(),
                }));
            effects.disconnect = Some("client requested quit");
            Ok(effects)
        }
        _ => {
            if kind == ChatKind::Shout {
                let event = Arc::new(ClusterEvent::GlobalShout(GlobalShoutEvent {
                    from_player_name: state.player_name.clone(),
                    from_empire: state.player_empire,
                    message: text,
                }));
                if let Err(err) = state.ctx.cluster_events.publish(event).await {
                    warn!(error = ?err, "Failed to broadcast global shout");
                }
            } else {
                let _ = state
                    .ctx
                    .map_events
                    .try_send(LocalMapInbound::ClientIntent {
                        msg: ClientIntentMsg {
                            player_id: state.player_id,
                            intent: ClientIntent::Chat { message },
                        },
                    });
            }
            Ok(PhaseEffects::empty())
        }
    }
}
