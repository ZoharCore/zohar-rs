use crate::bridge::InboundEvent;
use bevy::prelude::*;
use crossbeam_channel::Receiver;
use rand::rngs::SmallRng;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::oneshot;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::PlayerId;
use zohar_map_port::ClientTimestamp;

use super::time::SimInstant;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Resource))]
#[derive(Resource, Default)]
pub struct PlayerCount(pub u32);

#[derive(Resource)]
pub(crate) struct NetworkBridgeRx {
    pub(crate) inbound_rx: Receiver<InboundEvent>,
}

#[derive(Resource, Default)]
pub struct StartupReadySignal(pub(crate) Mutex<Option<oneshot::Sender<()>>>);

impl StartupReadySignal {
    pub fn new(tx: oneshot::Sender<()>) -> Self {
        Self(Mutex::new(Some(tx)))
    }
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(from_reflect = false))]
#[derive(Resource)]
pub(crate) struct RuntimeState {
    pub(crate) next_net_id: u32,
    pub(crate) next_pack_id: u32,
    pub(crate) map_entity: Option<Entity>,
    pub(crate) is_dirty: bool,
    pub(crate) sim_now: SimInstant,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub(crate) packet_time_start: Instant,
    #[cfg_attr(feature = "admin-brp", reflect(ignore))]
    pub(crate) rng: SmallRng,
}

impl RuntimeState {
    pub(crate) fn packet_now(&self) -> ClientTimestamp {
        self.sim_now.to_client_timestamp()
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            next_net_id: 0,
            next_pack_id: 0,
            map_entity: None,
            is_dirty: false,
            sim_now: SimInstant::ZERO,
            packet_time_start: Instant::now(),
            rng: rand::make_rng(),
        }
    }
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Resource))]
#[derive(Resource, Default)]
pub(crate) struct PlayerIndex(pub(crate) HashMap<PlayerId, Entity>);

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Resource))]
#[derive(Resource, Default)]
pub(crate) struct NetEntityIndex(pub(crate) HashMap<EntityId, Entity>);

pub(crate) fn next_entity_id(state: &mut RuntimeState) -> EntityId {
    state.next_net_id = state.next_net_id.wrapping_add(1);
    if state.next_net_id == 0 {
        state.next_net_id = 1;
    }
    EntityId(state.next_net_id)
}
