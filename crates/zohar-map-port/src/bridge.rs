use zohar_domain::Empire;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::{PlayerId, PlayerRuntimeEpoch};

use crate::messages::ClientIntent;

#[derive(Debug)]
pub struct EnterMsg {
    pub player_id: PlayerId,
    pub player_net_id: EntityId,
    pub runtime_epoch: PlayerRuntimeEpoch,
    pub initial_pos: LocalPos,
    pub appearance: PlayerAppearance,
}

#[derive(Debug)]
pub struct LeaveMsg {
    pub player_id: PlayerId,
    pub player_net_id: EntityId,
}

#[derive(Debug)]
pub struct ClientIntentMsg {
    pub player_id: PlayerId,
    pub intent: ClientIntent,
}

#[derive(Debug)]
pub struct GlobalShoutMsg {
    pub from_player_name: String,
    pub from_empire: Empire,
    pub message_bytes: Vec<u8>,
}
