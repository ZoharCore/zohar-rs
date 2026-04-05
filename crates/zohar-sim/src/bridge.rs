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

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::Empire;
    use zohar_domain::appearance::PlayerAppearance;
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::player::PlayerId;
    use zohar_domain::entity::{EntityId, MovementKind};
    use zohar_map_port::{
        AttackIntent, AttackTargetIntent, ClientIntent, ClientTimestamp, Facing72, MovementArg,
    };

    #[tokio::test]
    async fn reserve_net_id_round_trip() {
        let (sender, rx) = inbound_channel(4);
        let (reply_tx, reply_rx) = oneshot::channel();
        sender
            .enqueue(InboundEvent::ReserveNetId { reply: reply_tx })
            .expect("enqueue reserve request");

        let InboundEvent::ReserveNetId { reply } = rx.recv().expect("event") else {
            panic!("expected ReserveNetId");
        };

        let _ = reply.send(EntityId(1234));
        assert_eq!(reply_rx.await.expect("reply"), EntityId(1234));
    }

    #[test]
    fn try_send_reports_disconnected_queue() {
        let (sender, rx) = inbound_channel(1);
        drop(rx);
        let err = sender
            .try_send_client_intent(ClientIntentMsg {
                player_id: PlayerId::from(1),
                intent: ClientIntent::Move(zohar_map_port::MoveIntent {
                    kind: MovementKind::Move,
                    arg: MovementArg::ZERO,
                    facing: Facing72::try_from(0).expect("valid facing"),
                    target: LocalPos::new(1.0, 2.0),
                    client_ts: ClientTimestamp::new(1),
                }),
            })
            .expect_err("enqueue should fail when receiver is dropped");
        assert!(
            err.to_string().contains("closed"),
            "expected closed queue error, got: {err}"
        );
    }

    #[test]
    fn enter_player_creates_private_outbound_channel() {
        let (sender, rx) = inbound_channel(1);
        let _player_rx = sender
            .enter_player(EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7),
                initial_pos: LocalPos::new(1.0, 2.0),
                appearance: PlayerAppearance {
                    name: "alice".into(),
                    class: zohar_domain::entity::player::PlayerClass::Warrior,
                    gender: zohar_domain::entity::player::PlayerGender::Male,
                    empire: Empire::Red,
                    body_part: 0,
                    level: 1,
                    move_speed: 100,
                    attack_speed: 100,
                    guild_id: 0,
                },
            })
            .expect("player enter should enqueue");

        let InboundEvent::PlayerEnter { msg, .. } = rx.recv().expect("event") else {
            panic!("expected PlayerEnter");
        };
        assert_eq!(msg.player_id, PlayerId::from(1));
    }

    #[test]
    fn leave_player_and_snapshot_round_trips_reply() {
        let (sender, rx) = inbound_channel(1);
        let wait = std::thread::spawn({
            let sender = sender.clone();
            move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                runtime.block_on(async {
                    sender
                        .leave_player_and_snapshot(LeaveMsg {
                            player_id: PlayerId::from(1),
                            player_net_id: EntityId(7),
                        })
                        .await
                })
            }
        });

        let InboundEvent::PlayerLeaveAndSnapshot { msg, reply } = rx.recv().expect("event") else {
            panic!("expected PlayerLeaveAndSnapshot");
        };
        assert_eq!(msg.player_id, PlayerId::from(1));
        let _ = reply.send(Ok(PlayerRuntimeSnapshot {
            id: PlayerId::from(1),
            map_key: "zohar_map_a1".to_string(),
            local_pos: LocalPos::new(1.0, 2.0),
        }));

        let snapshot = wait.join().expect("join").expect("snapshot reply");
        assert_eq!(snapshot.id, PlayerId::from(1));
    }

    #[test]
    fn global_shout_round_trips_as_typed_message() {
        let (sender, rx) = inbound_channel(1);
        sender
            .try_send_global_shout(GlobalShoutMsg {
                from_player_name: "alice".into(),
                from_empire: Empire::Yellow,
                message_bytes: b"hello".to_vec(),
            })
            .expect("enqueue shout");

        let InboundEvent::GlobalShout { msg } = rx.recv().expect("event") else {
            panic!("expected GlobalShout");
        };
        assert_eq!(msg.from_player_name, "alice");
        assert_eq!(msg.from_empire, Empire::Yellow);
        assert_eq!(msg.message_bytes, b"hello");
    }

    #[test]
    fn client_attack_intent_round_trips_semantically() {
        let (sender, rx) = inbound_channel(1);
        sender
            .try_send_client_intent(ClientIntentMsg {
                player_id: PlayerId::from(1),
                intent: ClientIntent::Attack(AttackTargetIntent {
                    target: EntityId(99),
                    attack: AttackIntent::Basic,
                }),
            })
            .expect("enqueue attack");

        let InboundEvent::ClientIntent { msg } = rx.recv().expect("event") else {
            panic!("expected ClientIntent");
        };
        assert!(matches!(
            msg.intent,
            ClientIntent::Attack(AttackTargetIntent {
                target: EntityId(99),
                attack: AttackIntent::Basic,
            })
        ));
    }
}
