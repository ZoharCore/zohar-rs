use super::super::types::PhaseResult;
use super::{InGameCtx, InGamePhaseEffects};
use tracing::warn;
use zohar_protocol::game_pkt::ingame::fishing::FishingC2s;

pub(super) async fn handle_packet(
    _packet: FishingC2s,
    _state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    warn!("Unhandled in-game fishing packet");
    Ok(InGamePhaseEffects::disconnect(
        "unhandled in-game fishing packet",
    ))
}
