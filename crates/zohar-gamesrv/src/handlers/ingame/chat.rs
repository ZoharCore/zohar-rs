use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use crate::adapters::{ToDomain, ToProtocol};
use crate::infra::{ClusterEvent, GlobalShoutEvent};
use std::sync::Arc;
use tracing::{info, warn};
use zohar_domain::Empire;
use zohar_domain::entity::EntityId;
use zohar_map_port::{ChatChannel, ChatIntent as PortChatIntent, ClientIntent, ClientIntentMsg};
use zohar_protocol::decode_cstr;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::chat::{ChatC2s, ChatS2c};
use zohar_protocol::game_pkt::{ChatKind, ZeroOpt};

pub(super) async fn handle_packet(
    packet: ChatC2s,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    match packet {
        ChatC2s::SubmitChatMessage { kind, message } => {
            let text = decode_cstr(&message);
            let cmd = text.trim().to_owned();
            match cmd.as_str() {
                // TODO: implement some clap-style safe typed command parser
                "/phase_select" => {
                    info!(kind = ?kind, "Returning to character select");
                    Ok(PhaseEffects::transition(()))
                }
                "/logout" => Ok(PhaseEffects {
                    send: vec![
                        ChatS2c::NotifyChatMessage {
                            kind: ChatKind::Info,
                            net_id: ZeroOpt::none(),
                            empire: ZeroOpt::none(),
                            message: b"Back to login window. Please wait.\0".to_vec(),
                        }
                        .into(),
                    ],
                    transition: None,
                    disconnect: Some("client requested logout"),
                }),
                "/quit" => Ok(PhaseEffects {
                    send: vec![
                        ChatS2c::NotifyChatMessage {
                            kind: ChatKind::Command,
                            net_id: ZeroOpt::none(),
                            empire: ZeroOpt::none(),
                            message: b"quit\0".to_vec(),
                        }
                        .into(),
                    ],
                    transition: None,
                    disconnect: Some("client requested quit"),
                }),
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
                            .try_send_client_intent(ClientIntentMsg {
                                player_id: state.player_id,
                                intent: ClientIntent::Chat(PortChatIntent {
                                    // TODO: only broadcast local speaking packets
                                    channel: kind.to_domain(),
                                    message,
                                }),
                            });
                    }
                    Ok(PhaseEffects::empty())
                }
            }
        }
    }
}

pub(super) fn encode_chat_event(
    channel: ChatChannel,
    sender_entity_id: Option<EntityId>,
    empire: Option<Empire>,
    message: Vec<u8>,
) -> Vec<InGameS2c> {
    vec![
        ChatS2c::NotifyChatMessage {
            kind: channel.to_protocol(),
            net_id: ZeroOpt::from(sender_entity_id.map(|id| id.to_protocol())),
            empire: ZeroOpt::from(empire.map(|empire| empire.to_protocol())),
            message,
        }
        .into(),
    ]
}
