use bevy::prelude::Resource;

use tokio::sync::{mpsc, oneshot};
use zohar_domain::PlayerExitKind;
use zohar_domain::entity::player::PlayerSnapshot;

pub type PlayerPersistenceResult = Result<(), String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveUrgency {
    Autosave,
    FlushNow,
}

#[derive(Debug)]
pub enum PlayerPersistenceRequest {
    SaveSnapshot {
        snapshot: PlayerSnapshot,
        urgency: SaveUrgency,
        reply: Option<oneshot::Sender<PlayerPersistenceResult>>,
    },
    CommitPlayerExit {
        exit_kind: PlayerExitKind,
        username: String,
        server_id: String,
        connection_id: String,
        snapshot: PlayerSnapshot,
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlayerPersistenceQueueError {
    #[error("player persistence queue is full")]
    Full,
    #[error("player persistence queue is closed")]
    Closed,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerPersistenceCoordinatorHandle {
    tx: Option<mpsc::Sender<PlayerPersistenceRequest>>,
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct PlayerPersistencePort {
    handle: PlayerPersistenceCoordinatorHandle,
}

impl PlayerPersistenceCoordinatorHandle {
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    pub fn try_schedule_autosave(
        &self,
        snapshot: PlayerSnapshot,
    ) -> Result<(), PlayerPersistenceQueueError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };

        tx.try_send(PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            urgency: SaveUrgency::Autosave,
            reply: None,
        })
        .map_err(map_queue_error)
    }

    pub fn try_schedule_flush(
        &self,
        snapshot: PlayerSnapshot,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Ok(()));
            return Ok(reply_rx);
        };

        tx.try_send(PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            urgency: SaveUrgency::FlushNow,
            reply: Some(reply_tx),
        })
        .map_err(map_queue_error)?;
        Ok(reply_rx)
    }

    pub async fn schedule_flush(
        &self,
        snapshot: PlayerSnapshot,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Ok(()));
            return Ok(reply_rx);
        };

        tx.send(PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            urgency: SaveUrgency::FlushNow,
            reply: Some(reply_tx),
        })
        .await
        .map_err(|_| PlayerPersistenceQueueError::Closed)?;
        Ok(reply_rx)
    }

    pub async fn commit_player_exit(
        &self,
        exit_kind: PlayerExitKind,
        username: impl Into<String>,
        server_id: impl Into<String>,
        connection_id: impl Into<String>,
        snapshot: PlayerSnapshot,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Ok(()));
            return Ok(reply_rx);
        };

        tx.send(PlayerPersistenceRequest::CommitPlayerExit {
            exit_kind,
            username: username.into(),
            server_id: server_id.into(),
            connection_id: connection_id.into(),
            snapshot,
            reply: reply_tx,
        })
        .await
        .map_err(|_| PlayerPersistenceQueueError::Closed)?;
        Ok(reply_rx)
    }
}

impl PlayerPersistencePort {
    pub(crate) fn new(handle: PlayerPersistenceCoordinatorHandle) -> Self {
        Self { handle }
    }

    pub(crate) fn handle(&self) -> &PlayerPersistenceCoordinatorHandle {
        &self.handle
    }
}

pub fn player_persistence_channel(
    buffer: usize,
) -> (
    PlayerPersistenceCoordinatorHandle,
    mpsc::Receiver<PlayerPersistenceRequest>,
) {
    let (tx, rx) = mpsc::channel(buffer.max(1));
    (PlayerPersistenceCoordinatorHandle { tx: Some(tx) }, rx)
}

fn map_queue_error(
    error: mpsc::error::TrySendError<PlayerPersistenceRequest>,
) -> PlayerPersistenceQueueError {
    match error {
        mpsc::error::TrySendError::Full(_) => PlayerPersistenceQueueError::Full,
        mpsc::error::TrySendError::Closed(_) => PlayerPersistenceQueueError::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::player::{
        CoreStatAllocations, PlayerId, PlayerPlaytime, PlayerProgressionSnapshot,
        PlayerRuntimeSnapshot, PlayerSnapshot,
    };

    fn snapshot(player_id: PlayerId) -> PlayerSnapshot {
        PlayerSnapshot {
            runtime: PlayerRuntimeSnapshot {
                id: player_id,
                runtime_epoch: Default::default(),
                map_key: "zohar_map_a1".to_string(),
                playtime: PlayerPlaytime::ZERO,
                current_hp: None,
                current_sp: None,
                current_stamina: None,
                local_pos: LocalPos::new(1.0, 2.0),
            },
            progression: PlayerProgressionSnapshot {
                core_stat_allocations: CoreStatAllocations::default(),
                stat_reset_count: 0,
            },
        }
    }

    #[test]
    fn explicit_flush_returns_full_when_queue_capacity_is_exhausted() {
        let (handle, mut rx) = player_persistence_channel(1);
        handle
            .try_schedule_autosave(snapshot(PlayerId::from(1)))
            .expect("enqueue autosave");

        let error = handle
            .try_schedule_flush(snapshot(PlayerId::from(2)))
            .expect_err("flush should fail fast when queue is full");
        assert_eq!(error, PlayerPersistenceQueueError::Full);

        match rx.try_recv().expect("autosave request") {
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot,
                urgency,
                reply,
            } => {
                assert_eq!(snapshot.player_id(), PlayerId::from(1));
                assert!(matches!(urgency, SaveUrgency::Autosave));
                assert!(reply.is_none());
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }
}
