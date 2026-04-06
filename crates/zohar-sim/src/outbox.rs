//! Player outbox for batching and coalescing events.
//!
//! The outbox pattern allows the map tick thread to queue events for players
//! without blocking, and coalesces duplicate movement updates.

use std::collections::{HashMap, VecDeque};

use tokio::sync::mpsc::error::TrySendError;
use tracing::warn;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::PlayerId;
use zohar_map_port::{MovementEvent, PlayerEvent};

/// Outbox for a single player, held by the map actor.
///
/// Queues reliable events and coalesces movement updates to reduce packet spam.
#[derive(Debug)]
pub(crate) struct PlayerOutbox {
    owner_player_id: Option<PlayerId>,
    events: VecDeque<PlayerEvent>,
    movement_latest_player: HashMap<EntityId, MovementEvent>,
    movement_remote: VecDeque<MovementEvent>,
    tx: tokio::sync::mpsc::Sender<PlayerEvent>,
}

/// Statistics from flushing an outbox.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PlayerOutboxStats {
    pub(crate) sent_reliable: usize,
    pub(crate) sent_movement: usize,
    pub(crate) total_events: usize,
}

impl PlayerOutbox {
    /// Create a new empty outbox.
    pub(crate) fn new(tx: tokio::sync::mpsc::Sender<PlayerEvent>) -> Self {
        Self {
            owner_player_id: None,
            events: VecDeque::new(),
            movement_latest_player: HashMap::new(),
            movement_remote: VecDeque::new(),
            tx,
        }
    }

    /// Attach owning player id for operational telemetry.
    pub(crate) fn set_owner_player_id(&mut self, player_id: PlayerId) {
        self.owner_player_id = Some(player_id);
    }

    /// Queue a reliable event (spawns, chat, etc).
    pub(crate) fn push_reliable(&mut self, event: PlayerEvent) {
        self.events.push_back(event);
    }

    /// Update the latest movement for an entity and optionally prioritize it.
    ///
    /// Prioritized updates are flushed first, which prevents remote player
    /// movement from being starved behind mob wander traffic.
    pub(crate) fn set_latest_movement_with_priority(
        &mut self,
        movement: MovementEvent,
        prioritize: bool,
    ) {
        if prioritize {
            self.movement_remote
                .retain(|queued| queued.entity_id != movement.entity_id);
            self.movement_latest_player
                .insert(movement.entity_id, movement);
        } else {
            self.movement_latest_player.remove(&movement.entity_id);
            self.push_remote_movement_update(movement);
        }
    }

    pub(crate) fn push_remote_movement(&mut self, movement: MovementEvent) {
        self.push_remote_movement_update(movement);
    }

    fn push_remote_movement_update(&mut self, movement: MovementEvent) {
        if self
            .movement_remote
            .back()
            .is_some_and(|last| last == &movement)
        {
            return;
        }
        self.movement_remote.push_back(movement);
    }

    pub(crate) fn flush(&mut self) -> PlayerOutboxStats {
        let mut stats = PlayerOutboxStats::default();

        while let Some(event) = self.events.pop_front() {
            match self.tx.try_send(event) {
                Ok(()) => {
                    stats.sent_reliable += 1;
                }
                Err(TrySendError::Full(event)) => {
                    self.events.push_front(event);
                    break;
                }
                Err(TrySendError::Closed(_)) => {
                    warn!(
                        player_id = ?self.owner_player_id,
                        reliable_backlog = self.events.len(),
                        movement_backlog = self.pending_movement_count(),
                        "Outbox receiver closed while sending reliable event"
                    );
                    self.events.clear();
                    self.movement_latest_player.clear();
                    self.movement_remote.clear();
                    stats.total_events = stats.sent_reliable + stats.sent_movement;
                    return stats;
                }
            }
        }

        match self.flush_movement_bucket(&mut stats) {
            MovementFlushOutcome::Complete => {}
            MovementFlushOutcome::Full => {
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
            MovementFlushOutcome::Closed => {
                warn!(
                    player_id = ?self.owner_player_id,
                    reliable_backlog = self.events.len(),
                    movement_backlog = self.pending_movement_count(),
                    "Outbox receiver closed while sending movement update"
                );
                self.movement_latest_player.clear();
                self.movement_remote.clear();
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
        }

        match self.flush_remote_movement_queue(&mut stats) {
            MovementFlushOutcome::Complete => {}
            MovementFlushOutcome::Full => {
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
            MovementFlushOutcome::Closed => {
                warn!(
                    player_id = ?self.owner_player_id,
                    reliable_backlog = self.events.len(),
                    movement_backlog = self.pending_movement_count(),
                    "Outbox receiver closed while sending movement update"
                );
                self.movement_latest_player.clear();
                self.movement_remote.clear();
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
        }

        stats.total_events = stats.sent_reliable + stats.sent_movement;
        stats
    }

    fn flush_movement_bucket(&mut self, stats: &mut PlayerOutboxStats) -> MovementFlushOutcome {
        let bucket = &mut self.movement_latest_player;
        let mut pending_movement = std::mem::take(bucket).into_iter();
        while let Some((_, movement)) = pending_movement.next() {
            match self.tx.try_send(PlayerEvent::EntityMove(movement.clone())) {
                Ok(()) => {
                    stats.sent_movement += 1;
                }
                Err(TrySendError::Full(_)) => {
                    bucket.insert(movement.entity_id, movement);
                    for (_, rest) in pending_movement {
                        bucket.insert(rest.entity_id, rest);
                    }
                    return MovementFlushOutcome::Full;
                }
                Err(TrySendError::Closed(_)) => {
                    bucket.clear();
                    return MovementFlushOutcome::Closed;
                }
            }
        }
        MovementFlushOutcome::Complete
    }

    fn flush_remote_movement_queue(
        &mut self,
        stats: &mut PlayerOutboxStats,
    ) -> MovementFlushOutcome {
        while let Some(movement) = self.movement_remote.pop_front() {
            match self.tx.try_send(PlayerEvent::EntityMove(movement.clone())) {
                Ok(()) => {
                    stats.sent_movement += 1;
                }
                Err(TrySendError::Full(_)) => {
                    self.movement_remote.push_front(movement);
                    return MovementFlushOutcome::Full;
                }
                Err(TrySendError::Closed(_)) => {
                    self.movement_remote.clear();
                    return MovementFlushOutcome::Closed;
                }
            }
        }
        MovementFlushOutcome::Complete
    }

    fn pending_movement_count(&self) -> usize {
        self.movement_latest_player.len() + self.movement_remote.len()
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
            && self.movement_latest_player.is_empty()
            && self.movement_remote.is_empty()
    }
}

enum MovementFlushOutcome {
    Complete,
    Full,
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::Empire;
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::MovementKind;
    use zohar_map_port::{
        ChatChannel, ClientTimestamp, Facing72, MovementArg, MovementEvent, PacketDuration,
    };

    fn movement(entity_id: EntityId, x: f32, ts: u32) -> MovementEvent {
        MovementEvent {
            entity_id,
            kind: MovementKind::Move,
            arg: MovementArg::ZERO,
            facing: Facing72::try_from(0).expect("valid facing"),
            position: LocalPos::new(x, 1.0),
            client_ts: ClientTimestamp::new(ts),
            duration: PacketDuration::new(80),
        }
    }

    #[tokio::test]
    async fn prioritized_movement_keeps_latest_under_backpressure() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(tx.clone());

        tx.try_send(PlayerEvent::Chat {
            channel: ChatChannel::Speak,
            sender_entity_id: None,
            empire: Some(Empire::Red),
            message: b"prefill".to_vec(),
        })
        .expect("prefill should fit");

        outbox.set_latest_movement_with_priority(movement(EntityId(101), 1.0, 1000), true);
        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 0);

        outbox.set_latest_movement_with_priority(movement(EntityId(101), 2.0, 2000), true);
        let _ = rx.recv().await;

        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 1);

        match rx.recv().await {
            Some(PlayerEvent::EntityMove(movement)) => {
                assert_eq!(movement.entity_id, EntityId(101));
                assert_eq!(movement.client_ts.get(), 2000);
                assert_eq!(movement.position.x, 2.0);
            }
            other => panic!("expected latest prioritized movement packet, got: {other:?}"),
        }
    }
}
