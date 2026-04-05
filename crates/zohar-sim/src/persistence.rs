use bevy::prelude::Resource;

use tokio::sync::{mpsc, oneshot};
use zohar_domain::entity::player::PlayerRuntimeSnapshot;

pub type PlayerPersistenceResult = Result<(), String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSaveKind {
    Autosave,
    ExplicitFlush,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticalPlayerOpRequest {
    Reserved,
}

#[derive(Debug)]
pub enum PlayerPersistenceRequest {
    SaveSnapshot {
        snapshot: PlayerRuntimeSnapshot,
        kind: SnapshotSaveKind,
        reply: Option<oneshot::Sender<PlayerPersistenceResult>>,
    },
    FinalizeDisconnect {
        username: String,
        server_id: String,
        connection_id: String,
        snapshot: PlayerRuntimeSnapshot,
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
    CommitCriticalOp {
        request: CriticalPlayerOpRequest,
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
        snapshot: PlayerRuntimeSnapshot,
    ) -> Result<(), PlayerPersistenceQueueError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };

        tx.try_send(PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            kind: SnapshotSaveKind::Autosave,
            reply: None,
        })
        .map_err(map_queue_error)
    }

    pub async fn schedule_flush(
        &self,
        snapshot: PlayerRuntimeSnapshot,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Ok(()));
            return Ok(reply_rx);
        };

        tx.send(PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            kind: SnapshotSaveKind::ExplicitFlush,
            reply: Some(reply_tx),
        })
        .await
        .map_err(|_| PlayerPersistenceQueueError::Closed)?;
        Ok(reply_rx)
    }

    pub async fn finalize_disconnect(
        &self,
        username: impl Into<String>,
        server_id: impl Into<String>,
        connection_id: impl Into<String>,
        snapshot: PlayerRuntimeSnapshot,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Ok(()));
            return Ok(reply_rx);
        };

        tx.send(PlayerPersistenceRequest::FinalizeDisconnect {
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

    pub async fn commit_critical_op(
        &self,
        request: CriticalPlayerOpRequest,
    ) -> Result<oneshot::Receiver<PlayerPersistenceResult>, PlayerPersistenceQueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let Some(tx) = &self.tx else {
            let _ = reply_tx.send(Err("critical player op persistence is disabled".to_string()));
            return Ok(reply_rx);
        };

        tx.send(PlayerPersistenceRequest::CommitCriticalOp {
            request,
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
    use tokio::time::{Duration, timeout};
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::player::PlayerId;

    fn snapshot(player_id: PlayerId) -> PlayerRuntimeSnapshot {
        PlayerRuntimeSnapshot {
            id: player_id,
            map_key: "zohar_map_a1".to_string(),
            local_pos: LocalPos::new(1.0, 2.0),
        }
    }

    #[tokio::test]
    async fn explicit_flush_waits_for_queue_capacity_instead_of_failing_when_full() {
        let (handle, mut rx) = player_persistence_channel(1);
        handle
            .try_schedule_autosave(snapshot(PlayerId::from(1)))
            .expect("enqueue autosave");

        let flush = tokio::spawn({
            let handle = handle.clone();
            async move { handle.schedule_flush(snapshot(PlayerId::from(2))).await }
        });

        timeout(Duration::from_millis(50), async {
            loop {
                if flush.is_finished() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect_err("flush should wait for queue capacity");

        let _ = rx.recv().await.expect("autosave request");

        let reply_rx = timeout(Duration::from_millis(200), flush)
            .await
            .expect("flush enqueue should complete once capacity is available")
            .expect("flush task join")
            .expect("flush enqueue should succeed");

        match rx.recv().await.expect("flush request") {
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot,
                kind,
                reply,
            } => {
                assert_eq!(snapshot.id, PlayerId::from(2));
                assert!(matches!(kind, SnapshotSaveKind::ExplicitFlush));
                assert!(reply.is_some());
            }
            other => panic!("unexpected request: {other:?}"),
        }

        drop(reply_rx);
    }
}
