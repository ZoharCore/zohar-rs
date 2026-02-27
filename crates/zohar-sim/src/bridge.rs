use crate::api::ClientIntent;
use crate::outbox::PlayerOutbox;
use anyhow::anyhow;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use tokio::sync::oneshot;
use zohar_domain::Empire;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::PlayerId;

#[derive(Debug)]
pub struct EnterMsg {
    pub player_id: PlayerId,
    pub player_net_id: EntityId,
    pub initial_pos: LocalPos,
    pub appearance: PlayerAppearance,
    pub outbox: PlayerOutbox,
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

#[derive(Debug)]
pub enum LocalMapInbound {
    ReserveNetId {
        reply: oneshot::Sender<EntityId>,
    },
    PlayerEnter {
        msg: EnterMsg,
    },
    PlayerLeave {
        msg: LeaveMsg,
    },
    ClientIntent {
        msg: ClientIntentMsg,
    },
    GlobalShout {
        from_player_name: String,
        from_empire: Empire,
        message_bytes: Vec<u8>,
    },
}

#[derive(Debug)]
pub enum InboundEvent {
    ReserveNetId { reply: oneshot::Sender<EntityId> },
    PlayerEnter { msg: EnterMsg },
    PlayerLeave { msg: LeaveMsg },
    ClientIntent { msg: ClientIntentMsg },
    GlobalShout { msg: GlobalShoutMsg },
}

#[derive(Clone)]
pub struct MapEventSender {
    inbound_tx: Sender<InboundEvent>,
}

impl MapEventSender {
    pub fn channel_pair(buffer: usize) -> (Self, Receiver<InboundEvent>) {
        let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(buffer.max(1));
        (Self { inbound_tx }, inbound_rx)
    }

    pub fn send(&self, event: LocalMapInbound) -> anyhow::Result<()> {
        let event = self.to_inbound(event);
        self.inbound_tx.send(event).map_err(enqueue_error)
    }

    pub fn try_send(&self, event: LocalMapInbound) -> anyhow::Result<()> {
        let event = self.to_inbound(event);
        self.inbound_tx.try_send(event).map_err(enqueue_try_error)
    }

    pub async fn reserve_net_id(&self) -> anyhow::Result<EntityId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(LocalMapInbound::ReserveNetId { reply: reply_tx })?;
        reply_rx
            .await
            .map_err(|_| anyhow!("map runtime dropped net id reservation reply"))
    }

    fn to_inbound(&self, event: LocalMapInbound) -> InboundEvent {
        match event {
            LocalMapInbound::ReserveNetId { reply } => InboundEvent::ReserveNetId { reply },
            LocalMapInbound::PlayerEnter { msg } => InboundEvent::PlayerEnter { msg },
            LocalMapInbound::PlayerLeave { msg } => InboundEvent::PlayerLeave { msg },
            LocalMapInbound::ClientIntent { msg } => InboundEvent::ClientIntent { msg },
            LocalMapInbound::GlobalShout {
                from_player_name,
                from_empire,
                message_bytes,
            } => InboundEvent::GlobalShout {
                msg: GlobalShoutMsg {
                    from_player_name,
                    from_empire,
                    message_bytes,
                },
            },
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ClientIntent;
    use zohar_domain::entity::EntityId;
    use zohar_domain::entity::player::PlayerId;

    #[tokio::test]
    async fn reserve_net_id_round_trip() {
        let (sender, rx) = MapEventSender::channel_pair(4);
        let (reply_tx, reply_rx) = oneshot::channel();
        sender
            .send(LocalMapInbound::ReserveNetId { reply: reply_tx })
            .expect("enqueue reserve request");

        let InboundEvent::ReserveNetId { reply } = rx.recv().expect("event") else {
            panic!("expected ReserveNetId");
        };

        let _ = reply.send(EntityId(1234));
        assert_eq!(reply_rx.await.expect("reply"), EntityId(1234));
    }

    #[test]
    fn try_send_reports_full_queue() {
        let (sender, _rx) = MapEventSender::channel_pair(1);
        let (reply_a, _reply_a_rx) = oneshot::channel();
        sender
            .send(LocalMapInbound::ReserveNetId { reply: reply_a })
            .expect("enqueue first");

        let (reply_b, _reply_b_rx) = oneshot::channel();
        let err = sender
            .try_send(LocalMapInbound::ReserveNetId { reply: reply_b })
            .expect_err("second enqueue should fail when queue is full");
        assert!(
            err.to_string().contains("full"),
            "expected full queue error, got: {err}"
        );
    }

    #[test]
    fn send_reports_disconnected_queue() {
        let (sender, rx) = MapEventSender::channel_pair(1);
        drop(rx);
        let err = sender
            .send(LocalMapInbound::ClientIntent {
                msg: ClientIntentMsg {
                    player_id: PlayerId::from(1),
                    intent: ClientIntent::Move {
                        entity_id: EntityId(7),
                        kind: zohar_domain::entity::MovementKind::Move,
                        arg: 0,
                        rot: 0,
                        x: 1.0,
                        y: 2.0,
                        ts: 1,
                    },
                },
            })
            .expect_err("enqueue should fail when receiver is dropped");
        assert!(
            err.to_string().contains("closed"),
            "expected closed queue error, got: {err}"
        );
    }
}
