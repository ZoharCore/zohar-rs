use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use tracing::warn;
use zohar_protocol::game_pkt::ingame::fishing::FishingC2s;

pub(super) async fn handle_packet(
    _packet: FishingC2s,
    _state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    warn!("Unhandled in-game fishing packet");
    Ok(PhaseEffects::disconnect("unhandled in-game fishing packet"))
}
