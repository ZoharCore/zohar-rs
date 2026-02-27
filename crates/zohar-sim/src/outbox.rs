//! Player outbox for batching and coalescing events.
//!
//! The outbox pattern allows the map tick thread to queue events for players
//! without blocking, and coalesces duplicate movement updates.

use crate::api::PlayerEvent;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::error::TrySendError;
use tracing::warn;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};

/// Messages that can be sent to a player actor from the map.
#[derive(Debug, Clone)]
pub enum PlayerMapEvent {
    /// A game event for the player.
    Event(PlayerEvent),
}

/// Outbox for a single player, held by the map actor.
///
/// Queues reliable events and coalesces movement updates to reduce packet spam.
#[derive(Debug)]
pub struct PlayerOutbox {
    owner_player_id: Option<PlayerId>,
    events: VecDeque<PlayerEvent>,
    movement_latest_player: HashMap<EntityId, MovementUpdate>,
    movement_latest: HashMap<EntityId, MovementUpdate>,
    tx: tokio::sync::mpsc::Sender<PlayerEvent>,
}

#[derive(Debug, Clone)]
struct MovementUpdate {
    entity_id: EntityId,
    kind: MovementKind,
    arg: u8,
    rot: u8,
    x: f32,
    y: f32,
    time: u32,
    duration: u32,
}

/// Statistics from flushing an outbox.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlayerOutboxStats {
    pub sent_reliable: usize,
    pub sent_movement: usize,
    pub total_events: usize,
}

impl PlayerOutbox {
    /// Create a new empty outbox.
    pub fn new(tx: tokio::sync::mpsc::Sender<PlayerEvent>) -> Self {
        Self {
            owner_player_id: None,
            events: VecDeque::new(),
            movement_latest_player: HashMap::new(),
            movement_latest: HashMap::new(),
            tx,
        }
    }

    /// Attach owning player id for operational telemetry.
    pub fn set_owner_player_id(&mut self, player_id: PlayerId) {
        self.owner_player_id = Some(player_id);
    }

    /// Queue a reliable event (spawns, chat, etc).
    pub fn push_reliable(&mut self, event: PlayerEvent) {
        self.events.push_back(event);
    }

    pub fn flush_reliable(&mut self, event: PlayerEvent) {
        self.events.push_back(event);
        self.flush();
    }

    /// Update the latest movement for an entity (coalesced - only latest sent).
    #[allow(clippy::too_many_arguments)]
    pub fn set_latest_movement(
        &mut self,
        entity_id: EntityId,
        kind: MovementKind,
        arg: u8,
        rot: u8,
        x: f32,
        y: f32,
        time: u32,
        duration: u32,
    ) {
        self.set_latest_movement_with_priority(
            entity_id, kind, arg, rot, x, y, time, duration, false,
        );
    }

    /// Update the latest movement for an entity and optionally prioritize it.
    ///
    /// Prioritized updates are flushed first, which prevents remote player
    /// movement from being starved behind mob wander traffic.
    #[allow(clippy::too_many_arguments)]
    pub fn set_latest_movement_with_priority(
        &mut self,
        entity_id: EntityId,
        kind: MovementKind,
        arg: u8,
        rot: u8,
        x: f32,
        y: f32,
        time: u32,
        duration: u32,
        prioritize: bool,
    ) {
        let update = MovementUpdate {
            entity_id,
            kind,
            arg,
            rot,
            x,
            y,
            time,
            duration,
        };
        if prioritize {
            self.movement_latest.remove(&entity_id);
            self.movement_latest_player.insert(entity_id, update);
        } else {
            self.movement_latest_player.remove(&entity_id);
            self.movement_latest.insert(entity_id, update);
        }
    }

    pub fn flush(&mut self) -> PlayerOutboxStats {
        let mut stats = PlayerOutboxStats::default();

        // 1. Send Reliable events
        while let Some(event) = self.events.pop_front() {
            match self.tx.try_send(event) {
                Ok(()) => {
                    stats.sent_reliable += 1;
                }
                Err(TrySendError::Full(event)) => {
                    // Preserve order: retry this event first on next flush.
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
                    self.movement_latest.clear();
                    stats.total_events = stats.sent_reliable + stats.sent_movement;
                    return stats;
                }
            }
        }

        // 2. Send prioritized player movement first.
        match self.flush_movement_bucket(&mut stats, true) {
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
                self.movement_latest.clear();
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
        }

        // 3. Send remaining movement updates (e.g. mobs).
        match self.flush_movement_bucket(&mut stats, false) {
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
                self.movement_latest.clear();
                stats.total_events = stats.sent_reliable + stats.sent_movement;
                return stats;
            }
        }

        stats.total_events = stats.sent_reliable + stats.sent_movement;
        stats
    }

    fn flush_movement_bucket(
        &mut self,
        stats: &mut PlayerOutboxStats,
        prioritize_players: bool,
    ) -> MovementFlushOutcome {
        let bucket = if prioritize_players {
            &mut self.movement_latest_player
        } else {
            &mut self.movement_latest
        };
        let mut pending_movement = std::mem::take(bucket).into_iter();
        while let Some((_, update)) = pending_movement.next() {
            let event = PlayerEvent::EntityMove {
                entity_id: update.entity_id,
                kind: update.kind,
                arg: update.arg,
                rot: update.rot,
                x: update.x,
                y: update.y,
                ts: update.time,
                duration: update.duration,
            };

            match self.tx.try_send(event) {
                Ok(()) => {
                    stats.sent_movement += 1;
                }
                Err(TrySendError::Full(_)) => {
                    // Keep current and remaining entity movement updates for retry.
                    bucket.insert(update.entity_id, update);
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

    fn pending_movement_count(&self) -> usize {
        self.movement_latest_player.len() + self.movement_latest.len()
    }

    /// Check if outbox is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
            && self.movement_latest_player.is_empty()
            && self.movement_latest.is_empty()
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

    #[tokio::test]
    async fn movement_is_retained_when_channel_is_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(tx.clone());

        tx.try_send(PlayerEvent::Chat {
            kind: 0,
            sender_entity_id: None,
            empire: None,
            message: b"prefill".to_vec(),
        })
        .expect("prefill should fit");

        outbox.set_latest_movement(
            EntityId(101),
            MovementKind::Move,
            0,
            7,
            10.0,
            20.0,
            1234,
            567,
        );

        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 0);
        assert!(!outbox.is_empty(), "movement should be retained for retry");

        let _ = rx.recv().await;

        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 1);
        assert!(outbox.is_empty());

        match rx.recv().await {
            Some(PlayerEvent::EntityMove {
                entity_id,
                ts,
                duration,
                ..
            }) => {
                assert_eq!(entity_id, EntityId(101));
                assert_eq!(ts, 1234);
                assert_eq!(duration, 567);
            }
            other => panic!("expected retained movement packet, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn prioritized_movement_keeps_latest_under_backpressure() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(tx.clone());

        tx.try_send(PlayerEvent::Chat {
            kind: 0,
            sender_entity_id: None,
            empire: None,
            message: b"prefill".to_vec(),
        })
        .expect("prefill should fit");

        outbox.set_latest_movement_with_priority(
            EntityId(101),
            MovementKind::Move,
            0,
            0,
            1.0,
            1.0,
            1000,
            80,
            true,
        );

        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 0);
        assert!(!outbox.is_empty(), "movement should be retained for retry");

        outbox.set_latest_movement_with_priority(
            EntityId(101),
            MovementKind::Move,
            0,
            0,
            2.0,
            2.0,
            2000,
            80,
            true,
        );

        let _ = rx.recv().await;

        let stats = outbox.flush();
        assert_eq!(stats.sent_movement, 1);

        match rx.recv().await {
            Some(PlayerEvent::EntityMove {
                entity_id,
                ts,
                x,
                y,
                ..
            }) => {
                assert_eq!(entity_id, EntityId(101));
                assert_eq!(ts, 2000);
                assert_eq!(x, 2.0);
                assert_eq!(y, 2.0);
            }
            other => panic!("expected latest prioritized movement packet, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reliable_is_retained_when_channel_is_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(tx.clone());

        tx.try_send(PlayerEvent::Chat {
            kind: 0,
            sender_entity_id: None,
            empire: None,
            message: b"prefill".to_vec(),
        })
        .expect("prefill should fit");

        outbox.push_reliable(PlayerEvent::Chat {
            kind: 1,
            sender_entity_id: None,
            empire: None,
            message: b"queued".to_vec(),
        });

        let stats = outbox.flush();
        assert_eq!(stats.sent_reliable, 0);
        assert!(!outbox.is_empty(), "reliable event should be retained");

        let _ = rx.recv().await;

        let stats = outbox.flush();
        assert_eq!(stats.sent_reliable, 1);
        assert!(outbox.is_empty());

        match rx.recv().await {
            Some(PlayerEvent::Chat { kind, message, .. }) => {
                assert_eq!(kind, 1);
                assert_eq!(message, b"queued");
            }
            other => panic!("expected retained reliable packet, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn sustained_backpressure_does_not_starve_specific_entities() {
        let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(_tx);
        let entities = [EntityId(101), EntityId(102), EntityId(103)];
        let mut delivered = std::collections::HashMap::<EntityId, u32>::new();

        for tick in 0..40u32 {
            for entity_id in entities {
                outbox.set_latest_movement(
                    entity_id,
                    MovementKind::Move,
                    0,
                    0,
                    tick as f32,
                    0.0,
                    tick,
                    100,
                );
            }

            let _ = outbox.flush();
            if let Some(PlayerEvent::EntityMove { entity_id, .. }) = rx.recv().await {
                *delivered.entry(entity_id).or_insert(0) += 1;
            }
        }

        for entity_id in entities {
            assert!(
                delivered.get(&entity_id).copied().unwrap_or(0) > 0,
                "entity {entity_id:?} was starved under sustained backpressure"
            );
        }
    }

    #[tokio::test]
    async fn prioritized_player_movement_flushes_before_mobs() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut outbox = PlayerOutbox::new(tx);

        outbox.set_latest_movement(EntityId(201), MovementKind::Move, 0, 0, 1.0, 1.0, 1000, 80);
        outbox.set_latest_movement_with_priority(
            EntityId(101),
            MovementKind::Move,
            0,
            0,
            2.0,
            2.0,
            1001,
            80,
            true,
        );

        let _ = outbox.flush();
        match rx.recv().await {
            Some(PlayerEvent::EntityMove { entity_id, .. }) => {
                assert_eq!(
                    entity_id,
                    EntityId(101),
                    "prioritized player movement should flush first",
                );
            }
            other => panic!("expected movement packet, got: {other:?}"),
        }
    }
}
