use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use tracing::warn;
use zohar_protocol::game_pkt::ingame::trading::TradingC2s;

pub(super) async fn handle_packet(
    _packet: TradingC2s,
    _state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    warn!("Unhandled in-game trading packet");
    Ok(PhaseEffects::disconnect("unhandled in-game trading packet"))
}
