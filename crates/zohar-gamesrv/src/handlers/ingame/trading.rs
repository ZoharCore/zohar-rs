use super::super::types::PhaseResult;
use super::{InGameCtx, InGamePhaseEffects};
use tracing::warn;
use zohar_protocol::game_pkt::ingame::trading::TradingC2s;

pub(super) async fn handle_packet(
    _packet: TradingC2s,
    _state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    warn!("Unhandled in-game trading packet");
    Ok(InGamePhaseEffects::disconnect(
        "unhandled in-game trading packet",
    ))
}
