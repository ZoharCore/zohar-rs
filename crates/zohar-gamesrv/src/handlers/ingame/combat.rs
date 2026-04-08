use super::super::types::PhaseResult;
use super::{InGameCtx, InGamePhaseEffects};
use crate::adapters::ToDomain;
use tracing::{debug, warn};
use zohar_map_port::{
    AttackIntent as PortAttackIntent, AttackTargetIntent, ClientIntent, ClientIntentMsg,
};
use zohar_protocol::game_pkt::ZeroOpt;
use zohar_protocol::game_pkt::ingame::combat::CombatC2s;

pub(super) async fn handle_packet(
    packet: CombatC2s,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    match packet {
        CombatC2s::InputAttack {
            attack_type,
            target,
            _unknown,
        } => {
            let attack = match attack_type {
                ZeroOpt(None) => PortAttackIntent::Basic,
                ZeroOpt(Some(skill)) => PortAttackIntent::Skill(skill.to_domain()),
            };

            let intent_msg = ClientIntentMsg {
                player_id: state.player_id,
                intent: ClientIntent::Attack(AttackTargetIntent {
                    target: target.to_domain(),
                    attack,
                }),
            };
            if let Err(err) = state.ctx.map_events.try_send_client_intent(intent_msg) {
                warn!(
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    target = u32::from(target),
                    error = ?err,
                    "Failed to enqueue attack intent to map runtime"
                );
            }
            Ok(InGamePhaseEffects::empty())
        }
        CombatC2s::SignalTargetSwitch { target } => {
            debug!(
                player_id = ?state.player_id,
                map_id = state.map_id.get(),
                target = u32::from(target),
                "Received client target selection"
            );
            Ok(InGamePhaseEffects::empty())
        }
    }
}
