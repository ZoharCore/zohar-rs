use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use tracing::warn;
use zohar_protocol::game_pkt::ingame::guild::GuildC2s;

pub(super) async fn handle_packet(
    _packet: GuildC2s,
    _state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    warn!("Unhandled in-game guild packet");
    Ok(PhaseEffects::disconnect("unhandled in-game guild packet"))
}
