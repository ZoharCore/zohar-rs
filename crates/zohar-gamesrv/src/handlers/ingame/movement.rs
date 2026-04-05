use super::super::types::PhaseResult;
use super::{InGameCtx, PhaseEffects, ThisPhase};
use crate::ContentCoords;
use crate::adapters::{ToDomain, ToProtocol};
use tracing::warn;
use zohar_domain::MapId;
use zohar_map_port::{
    ClientIntent, ClientIntentMsg, ClientTimestamp, Facing72, MoveIntent, MovementArg,
    MovementEvent,
};
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::movement::{MovementC2s, MovementS2c};

pub(super) async fn handle_packet(
    packet: MovementC2s,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<PhaseEffects<ThisPhase>> {
    match packet {
        MovementC2s::InputMovement {
            kind,
            arg,
            rot,
            x,
            y,
            ts,
        } => {
            let kind = kind.to_domain();
            let packet_ts = u32::from(ts);
            let facing = Facing72::from_wrapped(rot);

            let Some(local_pos) = state.ctx.coords.world_wire_to_local(state.map_id, x, y) else {
                warn!(
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    wire_x = i32::from(x),
                    wire_y = i32::from(y),
                    "Ignoring out-of-bounds movement position"
                );
                return Ok(PhaseEffects::empty());
            };
            let intent_msg = ClientIntentMsg {
                player_id: state.player_id,
                intent: ClientIntent::Move(MoveIntent {
                    kind,
                    arg: MovementArg::from(arg),
                    facing,
                    target: local_pos,
                    // Preserve client-provided movement time (reference server behavior).
                    client_ts: ClientTimestamp::from(packet_ts),
                }),
            };
            if let Err(err) = state.ctx.map_events.try_send_client_intent(intent_msg) {
                warn!(
                    player_id = ?state.player_id,
                    map_id = state.map_id.get(),
                    kind = ?kind,
                    ts = packet_ts,
                    error = ?err,
                    "Failed to enqueue movement intent to map runtime"
                );
            }
            Ok(PhaseEffects::empty())
        }
    }
}

pub(super) fn encode_entity_move(
    movement: MovementEvent,
    map_id: MapId,
    coords: &ContentCoords,
) -> Vec<InGameS2c> {
    let local_pos = movement.position;
    let Some(world_pos) = coords.local_to_world(map_id, local_pos) else {
        warn!(
            map_id = map_id.get(),
            entity_id = movement.entity_id.0,
            kind = ?movement.kind,
            local_x = local_pos.x,
            local_y = local_pos.y,
            "Dropping movement packet due to out-of-bounds local position"
        );
        return Vec::new();
    };

    let (x, y) = world_pos.to_protocol();

    vec![
        MovementS2c::SyncEntityMovement {
            x,
            y,
            kind: movement.kind.to_protocol(),
            arg: movement.arg.get(),
            rot: movement.facing.get(),
            net_id: movement.entity_id.to_protocol(),
            // Preserve source timestamp to match reference movement semantics.
            ts: movement.client_ts.get().into(),
            duration: movement.duration.get().into(),
        }
        .into(),
    ]
}
