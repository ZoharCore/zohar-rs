use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::warn;
use zohar_db::{Game, GameDb, PlayersView, SessionsView};
use zohar_domain::entity::player::{PlayerId, PlayerRuntimeSnapshot};
use zohar_sim::{PlayerPersistenceRequest, PlayerPersistenceResult, SnapshotSaveKind};

const MAX_CONCURRENT_SAVES: usize = 8;
const AUTOSAVE_RETRY_DELAY: Duration = Duration::from_secs(1);

pub(crate) fn spawn_player_persistence_worker(
    runtime: &tokio::runtime::Runtime,
    db: Game,
    rx: mpsc::Receiver<PlayerPersistenceRequest>,
) {
    runtime.spawn(run_player_persistence_worker(db, rx));
}

async fn run_player_persistence_worker(db: Game, mut rx: mpsc::Receiver<PlayerPersistenceRequest>) {
    let mut lanes = HashMap::<PlayerId, PlayerPersistenceLane>::new();
    let mut in_flight = JoinSet::<CompletedDispatch>::new();
    let mut receiver_closed = false;

    loop {
        dispatch_ready_ops(&db, &mut lanes, &mut in_flight);

        if receiver_closed && in_flight.is_empty() && lanes.is_empty() {
            return;
        }

        let retry_deadline = next_retry_deadline(&lanes);

        tokio::select! {
            maybe_request = rx.recv(), if !receiver_closed => {
                match maybe_request {
                    Some(request) => handle_request(request, &mut lanes),
                    None => receiver_closed = true,
                }
            }
            maybe_done = in_flight.join_next(), if !in_flight.is_empty() => {
                if let Some(Ok(done)) = maybe_done {
                    handle_completed_dispatch(done, &mut lanes);
                }
            }
            _ = wait_for_retry(retry_deadline), if retry_deadline.is_some() => {}
        }
    }
}

async fn wait_for_retry(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    }
}

fn handle_request(
    request: PlayerPersistenceRequest,
    lanes: &mut HashMap<PlayerId, PlayerPersistenceLane>,
) {
    match request {
        PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            kind,
            reply,
        } => {
            let lane = lanes.entry(snapshot.id).or_default();
            match kind {
                SnapshotSaveKind::Autosave => {
                    if lane.has_pending_priority_op() {
                        return;
                    }
                    lane.pending_autosave = Some(snapshot);
                    lane.retry_at = None;
                }
                SnapshotSaveKind::ExplicitFlush => {
                    lane.pending_autosave = None;
                    lane.retry_at = None;
                    lane.pending_priority_ops
                        .push_back(PendingPriorityOp::FlushSnapshot {
                            snapshot,
                            reply: reply.expect(
                                "explicit snapshot flush requests must include a reply handle",
                            ),
                        });
                }
            }
        }
        PlayerPersistenceRequest::FinalizeDisconnect {
            username,
            server_id,
            connection_id,
            snapshot,
            reply,
        } => {
            let lane = lanes.entry(snapshot.id).or_default();
            lane.pending_autosave = None;
            lane.retry_at = None;
            lane.pending_priority_ops
                .push_back(PendingPriorityOp::FinalizeDisconnect {
                    username,
                    server_id,
                    connection_id,
                    snapshot,
                    reply,
                });
        }
        PlayerPersistenceRequest::CommitCriticalOp { request, reply } => {
            let _ = reply.send(Err(format!(
                "critical player op persistence is not implemented yet: {request:?}"
            )));
        }
    }
}

fn dispatch_ready_ops(
    db: &Game,
    lanes: &mut HashMap<PlayerId, PlayerPersistenceLane>,
    in_flight: &mut JoinSet<CompletedDispatch>,
) {
    if in_flight.len() >= MAX_CONCURRENT_SAVES {
        return;
    }

    for player_id in ready_player_ids(lanes, Instant::now()) {
        if in_flight.len() >= MAX_CONCURRENT_SAVES {
            break;
        }

        let Some(lane) = lanes.get_mut(&player_id) else {
            continue;
        };
        let Some(dispatch) = lane.next_dispatch() else {
            continue;
        };
        lane.in_flight = true;

        let db = db.clone();
        in_flight.spawn(async move {
            let DispatchOp { snapshot, kind } = dispatch;
            let result = match &kind {
                DispatchKind::Autosave | DispatchKind::FlushSnapshot { .. } => db
                    .players()
                    .save_runtime_state(&snapshot)
                    .await
                    .map_err(|error| error.to_string()),
                DispatchKind::FinalizeDisconnect {
                    username,
                    server_id,
                    connection_id,
                    ..
                } => db
                    .sessions()
                    .finalize_disconnect(username, server_id, connection_id, &snapshot)
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
            };

            CompletedDispatch {
                player_id,
                snapshot,
                kind,
                result,
            }
        });
    }
}

fn ready_player_ids(
    lanes: &HashMap<PlayerId, PlayerPersistenceLane>,
    now: Instant,
) -> Vec<PlayerId> {
    let mut ready = lanes
        .iter()
        .filter_map(|(player_id, lane)| {
            if lane.in_flight {
                return None;
            }
            if lane.has_pending_priority_op() {
                return Some((0_u8, *player_id));
            }
            let retry_ready = lane.retry_at.is_none_or(|retry_at| retry_at <= now);
            (retry_ready && lane.pending_autosave.is_some()).then_some((1_u8, *player_id))
        })
        .collect::<Vec<_>>();
    ready.sort_unstable_by_key(|(priority, _)| *priority);
    ready.into_iter().map(|(_, player_id)| player_id).collect()
}

fn next_retry_deadline(lanes: &HashMap<PlayerId, PlayerPersistenceLane>) -> Option<Instant> {
    lanes
        .values()
        .filter(|lane| !lane.in_flight)
        .filter(|lane| !lane.has_pending_priority_op())
        .filter(|lane| lane.pending_autosave.is_some())
        .filter_map(|lane| lane.retry_at)
        .min()
}

fn handle_completed_dispatch(
    completed: CompletedDispatch,
    lanes: &mut HashMap<PlayerId, PlayerPersistenceLane>,
) {
    let Some(lane) = lanes.get_mut(&completed.player_id) else {
        return;
    };

    lane.in_flight = false;

    match completed.kind {
        DispatchKind::Autosave => {
            if let Err(error) = &completed.result {
                if lane.has_pending_priority_op() {
                    warn!(
                        player_id = ?completed.player_id,
                        error = %error,
                        "Autosave persistence failed after a priority player write was queued; dropping autosave retry"
                    );
                    lane.retry_at = None;
                } else {
                    warn!(
                        player_id = ?completed.player_id,
                        error = %error,
                        "Autosave persistence failed; scheduling retry"
                    );
                    lane.pending_autosave = Some(completed.snapshot);
                    lane.retry_at = Some(Instant::now() + AUTOSAVE_RETRY_DELAY);
                }
            } else {
                lane.retry_at = None;
            }
        }
        DispatchKind::FlushSnapshot { reply } | DispatchKind::FinalizeDisconnect { reply, .. } => {
            let _ = reply.send(completed.result);
        }
    }

    if lane.is_idle() {
        lanes.remove(&completed.player_id);
    }
}

#[derive(Default)]
struct PlayerPersistenceLane {
    in_flight: bool,
    pending_autosave: Option<PlayerRuntimeSnapshot>,
    pending_priority_ops: VecDeque<PendingPriorityOp>,
    retry_at: Option<Instant>,
}

impl PlayerPersistenceLane {
    fn next_dispatch(&mut self) -> Option<DispatchOp> {
        if let Some(pending) = self.pending_priority_ops.pop_front() {
            return Some(match pending {
                PendingPriorityOp::FlushSnapshot { snapshot, reply } => DispatchOp {
                    snapshot,
                    kind: DispatchKind::FlushSnapshot { reply },
                },
                PendingPriorityOp::FinalizeDisconnect {
                    username,
                    server_id,
                    connection_id,
                    snapshot,
                    reply,
                } => DispatchOp {
                    snapshot,
                    kind: DispatchKind::FinalizeDisconnect {
                        username,
                        server_id,
                        connection_id,
                        reply,
                    },
                },
            });
        }

        self.pending_autosave.take().map(|snapshot| DispatchOp {
            snapshot,
            kind: DispatchKind::Autosave,
        })
    }

    fn has_pending_priority_op(&self) -> bool {
        !self.pending_priority_ops.is_empty()
    }

    fn is_idle(&self) -> bool {
        !self.in_flight && self.pending_autosave.is_none() && self.pending_priority_ops.is_empty()
    }
}

enum PendingPriorityOp {
    FlushSnapshot {
        snapshot: PlayerRuntimeSnapshot,
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
    FinalizeDisconnect {
        username: String,
        server_id: String,
        connection_id: String,
        snapshot: PlayerRuntimeSnapshot,
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
}

struct DispatchOp {
    snapshot: PlayerRuntimeSnapshot,
    kind: DispatchKind,
}

enum DispatchKind {
    Autosave,
    FlushSnapshot {
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
    FinalizeDisconnect {
        username: String,
        server_id: String,
        connection_id: String,
        reply: oneshot::Sender<PlayerPersistenceResult>,
    },
}

struct CompletedDispatch {
    player_id: PlayerId,
    snapshot: PlayerRuntimeSnapshot,
    kind: DispatchKind,
    result: PlayerPersistenceResult,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::coords::LocalPos;

    fn snapshot(player_id: PlayerId, x: f32, y: f32) -> PlayerRuntimeSnapshot {
        PlayerRuntimeSnapshot {
            id: player_id,
            map_key: "zohar_map_a1".to_string(),
            local_pos: LocalPos::new(x, y),
        }
    }

    #[test]
    fn autosave_requests_coalesce_per_player_lane() {
        let player_id = PlayerId::from(1);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 1.0, 2.0),
                kind: SnapshotSaveKind::Autosave,
                reply: None,
            },
            &mut lanes,
        );
        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 3.0, 4.0),
                kind: SnapshotSaveKind::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        assert_eq!(
            lane.pending_autosave
                .as_ref()
                .expect("pending autosave")
                .local_pos,
            LocalPos::new(3.0, 4.0)
        );
        assert!(lane.pending_priority_ops.is_empty());
    }

    #[test]
    fn explicit_flush_replaces_pending_autosave_and_preserves_reply() {
        let player_id = PlayerId::from(2);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 1.0, 2.0),
                kind: SnapshotSaveKind::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let (reply_tx, _reply_rx) = oneshot::channel();
        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 5.0, 6.0),
                kind: SnapshotSaveKind::ExplicitFlush,
                reply: Some(reply_tx),
            },
            &mut lanes,
        );

        let lane = lanes.get_mut(&player_id).expect("lane");
        assert!(lane.pending_autosave.is_none());

        let dispatch = lane.next_dispatch().expect("dispatch");
        assert_eq!(dispatch.snapshot.local_pos, LocalPos::new(5.0, 6.0));
        assert!(matches!(dispatch.kind, DispatchKind::FlushSnapshot { .. }));
    }

    #[test]
    fn finalize_disconnect_replaces_pending_autosave_and_preserves_reply() {
        let player_id = PlayerId::from(3);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 1.0, 2.0),
                kind: SnapshotSaveKind::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let disconnect_snapshot = snapshot(player_id, 7.0, 8.0);
        let (reply_tx, _reply_rx) = oneshot::channel();
        handle_request(
            PlayerPersistenceRequest::FinalizeDisconnect {
                username: "alice".to_string(),
                server_id: "ch1-core".to_string(),
                connection_id: "conn-1".to_string(),
                snapshot: disconnect_snapshot,
                reply: reply_tx,
            },
            &mut lanes,
        );

        let lane = lanes.get_mut(&player_id).expect("lane");
        assert!(lane.pending_autosave.is_none());

        let dispatch = lane.next_dispatch().expect("dispatch");
        assert_eq!(dispatch.snapshot.local_pos, LocalPos::new(7.0, 8.0));
        match dispatch.kind {
            DispatchKind::FinalizeDisconnect {
                username,
                server_id,
                connection_id,
                ..
            } => {
                assert_eq!(username, "alice");
                assert_eq!(server_id, "ch1-core");
                assert_eq!(connection_id, "conn-1");
            }
            DispatchKind::Autosave | DispatchKind::FlushSnapshot { .. } => {
                panic!("expected finalize disconnect dispatch")
            }
        }
    }

    #[test]
    fn failed_autosave_does_not_retry_after_priority_op_is_queued() {
        let player_id = PlayerId::from(4);
        let autosave_snapshot = snapshot(player_id, 1.0, 2.0);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: autosave_snapshot.clone(),
                kind: SnapshotSaveKind::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let lane = lanes.get_mut(&player_id).expect("lane");
        let dispatch = lane.next_dispatch().expect("autosave dispatch");
        assert!(matches!(dispatch.kind, DispatchKind::Autosave));
        lane.in_flight = true;

        let (reply_tx, _reply_rx) = oneshot::channel();
        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot(player_id, 5.0, 6.0),
                kind: SnapshotSaveKind::ExplicitFlush,
                reply: Some(reply_tx),
            },
            &mut lanes,
        );

        handle_completed_dispatch(
            CompletedDispatch {
                player_id,
                snapshot: autosave_snapshot,
                kind: DispatchKind::Autosave,
                result: Err("autosave failed".to_string()),
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        assert!(lane.pending_autosave.is_none());
        assert!(lane.retry_at.is_none());
        assert!(lane.has_pending_priority_op());
    }

    #[test]
    fn ready_players_prioritize_disconnects_and_flushes_over_autosaves() {
        let now = Instant::now();
        let autosave_player = PlayerId::from(20);
        let flush_player = PlayerId::from(21);
        let disconnect_player = PlayerId::from(22);

        let mut lanes = HashMap::new();
        lanes.insert(
            autosave_player,
            PlayerPersistenceLane {
                in_flight: false,
                pending_autosave: Some(snapshot(autosave_player, 1.0, 2.0)),
                pending_priority_ops: VecDeque::new(),
                retry_at: None,
            },
        );
        lanes.insert(
            flush_player,
            PlayerPersistenceLane {
                in_flight: false,
                pending_autosave: None,
                pending_priority_ops: VecDeque::from([PendingPriorityOp::FlushSnapshot {
                    snapshot: snapshot(flush_player, 3.0, 4.0),
                    reply: oneshot::channel().0,
                }]),
                retry_at: None,
            },
        );
        lanes.insert(
            disconnect_player,
            PlayerPersistenceLane {
                in_flight: false,
                pending_autosave: None,
                pending_priority_ops: VecDeque::from([PendingPriorityOp::FinalizeDisconnect {
                    username: "bob".to_string(),
                    server_id: "ch1-core".to_string(),
                    connection_id: "conn-2".to_string(),
                    snapshot: snapshot(disconnect_player, 5.0, 6.0),
                    reply: oneshot::channel().0,
                }]),
                retry_at: None,
            },
        );

        let ready = ready_player_ids(&lanes, now);
        assert_eq!(ready.len(), 3);
        assert!(ready[..2].contains(&flush_player));
        assert!(ready[..2].contains(&disconnect_player));
        assert_eq!(ready[2], autosave_player);
    }
}
