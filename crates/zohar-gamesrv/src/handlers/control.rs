use super::types::PhaseResult;
use std::time::Instant;
use tracing::debug;
use zohar_protocol::game_pkt::{ControlC2s, ControlS2c};
use zohar_protocol::handshake::{HandshakeOutcome, HandshakeState};

pub(crate) struct ControlOutcome<S2c> {
    pub send: Vec<S2c>,
}

pub(crate) enum ControlDecision<S2c> {
    Handled(ControlOutcome<S2c>),
    Reject(&'static str),
}

pub(crate) fn handle_session_control<S2c>(
    control: ControlC2s,
    now: Instant,
    handshake: &mut HandshakeState,
) -> PhaseResult<ControlDecision<S2c>>
where
    S2c: From<ControlS2c>,
{
    match control {
        ControlC2s::HeartbeatResponse => Ok(ControlDecision::Handled(ControlOutcome {
            send: Vec::new(),
        })),
        ControlC2s::RequestTimeSync { data } => {
            let outcome = handshake.handle(data, now)?;
            let mut send = Vec::new();
            match outcome {
                HandshakeOutcome::SendTimeSyncAck => {
                    debug!("Time-sync request accepted; sending 0xFC ack");
                    send.push(ControlS2c::TimeSyncResponse.into());
                }
                HandshakeOutcome::SendHandshakeSync(data) => {
                    debug!("Time-sync drift detected; sending handshake sync payload");
                    send.push(ControlS2c::RequestHandshake { data }.into());
                }
                HandshakeOutcome::CompletedInitial => {}
            }
            Ok(ControlDecision::Handled(ControlOutcome { send }))
        }
        ControlC2s::HandshakeResponse { .. } => Ok(ControlDecision::Reject(
            "SyncReply is only valid during handshake",
        )),
    }
}
