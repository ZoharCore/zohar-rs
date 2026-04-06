use anyhow::anyhow;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use tokio::sync::oneshot;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::PlayerRuntimeSnapshot;
use zohar_map_port::{ClientIntentMsg, EnterMsg, GlobalShoutMsg, LeaveMsg, PlayerEvent};

use crate::outbox::PlayerOutbox;

const PLAYER_EVENT_BUFFER: usize = 256;

#[derive(Debug)]
pub(crate) enum InboundEvent {
    ReserveNetId {
        reply: oneshot::Sender<EntityId>,
    },
    PlayerEnter {
        msg: EnterMsg,
        outbox: PlayerOutbox,
    },
    PlayerLeave {
        msg: LeaveMsg,
    },
    PlayerLeaveAndSnapshot {
        msg: LeaveMsg,
        reply: oneshot::Sender<anyhow::Result<PlayerRuntimeSnapshot>>,
    },
    ClientIntent {
        msg: ClientIntentMsg,
    },
    GlobalShout {
        msg: GlobalShoutMsg,
    },
}

pub(crate) fn inbound_channel(buffer: usize) -> (MapEventSender, Receiver<InboundEvent>) {
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(buffer.max(1));
    (MapEventSender { inbound_tx }, inbound_rx)
}

#[derive(Clone)]
pub struct MapEventSender {
    inbound_tx: Sender<InboundEvent>,
}

impl MapEventSender {
    pub fn send_player_leave(&self, msg: LeaveMsg) -> anyhow::Result<()> {
        self.enqueue(InboundEvent::PlayerLeave { msg })
    }

    pub async fn leave_player_and_snapshot(
        &self,
        msg: LeaveMsg,
    ) -> anyhow::Result<PlayerRuntimeSnapshot> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.enqueue(InboundEvent::PlayerLeaveAndSnapshot {
            msg,
            reply: reply_tx,
        })?;
        reply_rx
            .await
            .map_err(|_| anyhow!("map runtime dropped player leave+snapshot reply"))?
    }

    pub fn try_send_client_intent(&self, msg: ClientIntentMsg) -> anyhow::Result<()> {
        self.try_enqueue(InboundEvent::ClientIntent { msg })
    }

    pub fn try_send_global_shout(&self, msg: GlobalShoutMsg) -> anyhow::Result<()> {
        self.try_enqueue(InboundEvent::GlobalShout { msg })
    }

    pub fn enter_player(
        &self,
        msg: EnterMsg,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<PlayerEvent>> {
        let (player_tx, player_rx) = tokio::sync::mpsc::channel(PLAYER_EVENT_BUFFER);
        self.enqueue(InboundEvent::PlayerEnter {
            msg,
            outbox: PlayerOutbox::new(player_tx),
        })?;
        Ok(player_rx)
    }

    pub async fn reserve_net_id(&self) -> anyhow::Result<EntityId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.enqueue(InboundEvent::ReserveNetId { reply: reply_tx })?;
        reply_rx
            .await
            .map_err(|_| anyhow!("map runtime dropped net id reservation reply"))
    }

    fn enqueue(&self, event: InboundEvent) -> anyhow::Result<()> {
        self.inbound_tx.send(event).map_err(enqueue_error)
    }

    fn try_enqueue(&self, event: InboundEvent) -> anyhow::Result<()> {
        self.inbound_tx.try_send(event).map_err(enqueue_try_error)
    }
}

fn enqueue_try_error(err: TrySendError<InboundEvent>) -> anyhow::Error {
    match err {
        TrySendError::Full(_) => anyhow!("map runtime inbound queue is full/overloaded"),
        TrySendError::Disconnected(_) => anyhow!("map runtime inbound queue is closed/unavailable"),
    }
}

fn enqueue_error(err: crossbeam_channel::SendError<InboundEvent>) -> anyhow::Error {
    anyhow!("map runtime inbound queue is closed/unavailable: {err}")
}
