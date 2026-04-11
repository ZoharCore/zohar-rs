use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::warn;
use zohar_db::{Game, GameDb, PlayerStatesView, PlayerWriteOutcome, SessionsView};
use zohar_domain::{
    PlayerExitKind,
    entity::player::{PlayerId, PlayerSnapshot},
};
use zohar_sim::{PlayerPersistenceRequest, PlayerPersistenceResult, SaveUrgency};

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

async fn save_player_snapshot(
    db: &Game,
    snapshot: &PlayerSnapshot,
) -> Result<PlayerWriteOutcome, String> {
    db.player_states()
        .save_player_snapshot(snapshot)
        .await
        .map_err(|error| error.to_string())
}

fn handle_request(
    request: PlayerPersistenceRequest,
    lanes: &mut HashMap<PlayerId, PlayerPersistenceLane>,
) {
    match request {
        PlayerPersistenceRequest::SaveSnapshot {
            snapshot,
            urgency,
            reply,
        } => {
            let lane = lanes.entry(snapshot.player_id()).or_default();
            if lane.pending_exit.is_some() {
                if let Some(reply) = reply {
                    let _ = reply.send(Err("player exit is already pending".to_string()));
                }
                return;
            }

            lane.retry_at = None;
            match &mut lane.pending_save {
                Some(pending) => {
                    pending.snapshot = snapshot;
                    if matches!(urgency, SaveUrgency::FlushNow) {
                        pending.urgency = SaveUrgency::FlushNow;
                        if let Some(reply) = reply {
                            if pending.reply.is_some() {
                                let _ =
                                    reply.send(Err("player flush is already pending".to_string()));
                            } else {
                                pending.reply = Some(reply);
                            }
                        }
                    }
                }
                None => {
                    lane.pending_save = Some(PendingSave {
                        snapshot,
                        urgency,
                        reply,
                    });
                }
            }
        }
        PlayerPersistenceRequest::CommitPlayerExit {
            exit_kind,
            username,
            server_id,
            connection_id,
            snapshot,
            reply,
        } => {
            let lane = lanes.entry(snapshot.player_id()).or_default();
            lane.pending_save = None;
            lane.retry_at = None;
            lane.pending_exit = Some(PendingExit {
                exit_kind,
                username,
                server_id,
                connection_id,
                snapshot,
                reply,
            });
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
            let DispatchOp { player_id, kind } = dispatch;
            let result = match &kind {
                DispatchKind::Save {
                    snapshot, urgency, ..
                } => save_player_snapshot(&db, snapshot)
                    .await
                    .and_then(|outcome| match (urgency, outcome) {
                        (SaveUrgency::Autosave, PlayerWriteOutcome::Saved)
                        | (SaveUrgency::Autosave, PlayerWriteOutcome::StaleOwner)
                        | (SaveUrgency::FlushNow, PlayerWriteOutcome::Saved) => Ok(()),
                        (SaveUrgency::FlushNow, PlayerWriteOutcome::StaleOwner) => Err(
                            "owned player state flush rejected because runtime ownership moved"
                                .to_string(),
                        ),
                    }),
                DispatchKind::CommitPlayerExit { exit, .. } => db
                    .sessions()
                    .commit_player_exit(
                        exit.exit_kind,
                        &exit.username,
                        &exit.server_id,
                        &exit.connection_id,
                        &exit.snapshot,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
            };

            CompletedDispatch {
                player_id,
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
            if lane.pending_exit.is_some() {
                return Some((0_u8, *player_id));
            }
            let save = lane.pending_save.as_ref()?;
            let priority = match save.urgency {
                SaveUrgency::FlushNow => 1_u8,
                SaveUrgency::Autosave => {
                    if lane.retry_at.is_some_and(|retry_at| retry_at > now) {
                        return None;
                    }
                    2_u8
                }
            };
            Some((priority, *player_id))
        })
        .collect::<Vec<_>>();
    ready.sort_unstable_by_key(|(priority, _)| *priority);
    ready.into_iter().map(|(_, player_id)| player_id).collect()
}

fn next_retry_deadline(lanes: &HashMap<PlayerId, PlayerPersistenceLane>) -> Option<Instant> {
    lanes
        .values()
        .filter(|lane| !lane.in_flight)
        .filter(|lane| lane.pending_exit.is_none())
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
        DispatchKind::Save {
            snapshot,
            urgency,
            reply,
        } => match urgency {
            SaveUrgency::Autosave => {
                if let Err(error) = &completed.result {
                    if lane.pending_exit.is_some() || lane.pending_save.is_some() {
                        warn!(
                            player_id = ?completed.player_id,
                            error = %error,
                            "Autosave failed after a newer player save was queued; dropping retry"
                        );
                        lane.retry_at = None;
                    } else {
                        warn!(
                            player_id = ?completed.player_id,
                            error = %error,
                            "Autosave persistence failed; scheduling retry"
                        );
                        lane.pending_save = Some(PendingSave {
                            snapshot,
                            urgency,
                            reply,
                        });
                        lane.retry_at = Some(Instant::now() + AUTOSAVE_RETRY_DELAY);
                    }
                } else {
                    lane.retry_at = None;
                }
            }
            SaveUrgency::FlushNow => {
                if let Some(reply) = reply {
                    let _ = reply.send(completed.result);
                }
            }
        },
        DispatchKind::CommitPlayerExit { exit } => {
            let _ = exit.reply.send(completed.result);
        }
    }

    if lane.is_idle() {
        lanes.remove(&completed.player_id);
    }
}

#[derive(Default)]
struct PlayerPersistenceLane {
    in_flight: bool,
    pending_save: Option<PendingSave>,
    pending_exit: Option<PendingExit>,
    retry_at: Option<Instant>,
}

impl PlayerPersistenceLane {
    fn next_dispatch(&mut self) -> Option<DispatchOp> {
        if let Some(exit) = self.pending_exit.take() {
            return Some(DispatchOp {
                player_id: exit.snapshot.player_id(),
                kind: DispatchKind::CommitPlayerExit { exit },
            });
        }

        self.pending_save.take().map(|save| DispatchOp {
            player_id: save.snapshot.player_id(),
            kind: DispatchKind::Save {
                snapshot: save.snapshot,
                urgency: save.urgency,
                reply: save.reply,
            },
        })
    }

    fn is_idle(&self) -> bool {
        !self.in_flight && self.pending_save.is_none() && self.pending_exit.is_none()
    }
}

struct PendingSave {
    snapshot: PlayerSnapshot,
    urgency: SaveUrgency,
    reply: Option<oneshot::Sender<PlayerPersistenceResult>>,
}

struct PendingExit {
    exit_kind: PlayerExitKind,
    username: String,
    server_id: String,
    connection_id: String,
    snapshot: PlayerSnapshot,
    reply: oneshot::Sender<PlayerPersistenceResult>,
}

struct DispatchOp {
    player_id: PlayerId,
    kind: DispatchKind,
}

enum DispatchKind {
    Save {
        snapshot: PlayerSnapshot,
        urgency: SaveUrgency,
        reply: Option<oneshot::Sender<PlayerPersistenceResult>>,
    },
    CommitPlayerExit {
        exit: PendingExit,
    },
}

struct CompletedDispatch {
    player_id: PlayerId,
    kind: DispatchKind,
    result: PlayerPersistenceResult,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::player::{
        CoreStatAllocations, PlayerPlaytime, PlayerProgressionSnapshot, PlayerRuntimeSnapshot,
        PlayerSnapshot,
    };

    fn player_snapshot(player_id: PlayerId, x: f32, y: f32) -> PlayerSnapshot {
        PlayerSnapshot {
            runtime: PlayerRuntimeSnapshot {
                id: player_id,
                runtime_epoch: Default::default(),
                map_key: "zohar_map_a1".to_string(),
                playtime: PlayerPlaytime::ZERO,
                current_hp: None,
                current_sp: None,
                current_stamina: None,
                local_pos: LocalPos::new(x, y),
            },
            progression: PlayerProgressionSnapshot {
                core_stat_allocations: CoreStatAllocations::default(),
                stat_reset_count: 0,
            },
        }
    }

    fn exit_snapshot(player_id: PlayerId, x: f32, y: f32) -> PlayerSnapshot {
        player_snapshot(player_id, x, y)
    }

    #[test]
    fn autosaves_coalesce_to_latest_state() {
        let player_id = PlayerId::from(1);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: player_snapshot(player_id, 1.0, 2.0),
                urgency: SaveUrgency::Autosave,
                reply: None,
            },
            &mut lanes,
        );
        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: player_snapshot(player_id, 3.0, 4.0),
                urgency: SaveUrgency::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        assert_eq!(
            lane.pending_save
                .as_ref()
                .expect("pending save")
                .snapshot
                .runtime
                .local_pos,
            LocalPos::new(3.0, 4.0)
        );
    }

    #[test]
    fn flush_upgrades_pending_autosave_without_second_lane() {
        let player_id = PlayerId::from(2);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: player_snapshot(player_id, 1.0, 2.0),
                urgency: SaveUrgency::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let (reply_tx, _reply_rx) = oneshot::channel();
        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: player_snapshot(player_id, 5.0, 6.0),
                urgency: SaveUrgency::FlushNow,
                reply: Some(reply_tx),
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        let pending = lane.pending_save.as_ref().expect("pending save");
        assert!(matches!(pending.urgency, SaveUrgency::FlushNow));
        assert_eq!(pending.snapshot.runtime.local_pos, LocalPos::new(5.0, 6.0));
        assert!(pending.reply.is_some());
    }

    #[test]
    fn exit_replaces_pending_save() {
        let player_id = PlayerId::from(3);
        let mut lanes = HashMap::new();

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: player_snapshot(player_id, 1.0, 2.0),
                urgency: SaveUrgency::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let (reply_tx, _reply_rx) = oneshot::channel();
        handle_request(
            PlayerPersistenceRequest::CommitPlayerExit {
                exit_kind: PlayerExitKind::Disconnect,
                username: "alice".to_string(),
                server_id: "ch1-core".to_string(),
                connection_id: "conn-1".to_string(),
                snapshot: exit_snapshot(player_id, 7.0, 8.0),
                reply: reply_tx,
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        assert!(lane.pending_save.is_none());
        assert!(lane.pending_exit.is_some());
    }

    #[test]
    fn failed_autosave_retries_when_no_newer_state_arrives() {
        let player_id = PlayerId::from(4);
        let mut lanes = HashMap::new();
        let snapshot = player_snapshot(player_id, 1.0, 2.0);

        handle_request(
            PlayerPersistenceRequest::SaveSnapshot {
                snapshot: snapshot.clone(),
                urgency: SaveUrgency::Autosave,
                reply: None,
            },
            &mut lanes,
        );

        let lane = lanes.get_mut(&player_id).expect("lane");
        let dispatch = lane.next_dispatch().expect("dispatch");
        lane.in_flight = true;

        handle_completed_dispatch(
            CompletedDispatch {
                player_id,
                kind: dispatch.kind,
                result: Err("autosave failed".to_string()),
            },
            &mut lanes,
        );

        let lane = lanes.get(&player_id).expect("lane");
        assert_eq!(
            lane.pending_save
                .as_ref()
                .expect("retry save")
                .snapshot
                .runtime
                .local_pos,
            snapshot.runtime.local_pos
        );
        assert!(lane.retry_at.is_some());
    }

    #[test]
    fn ready_players_prioritize_exit_then_flush_then_autosave() {
        let now = Instant::now();
        let autosave_player = PlayerId::from(20);
        let flush_player = PlayerId::from(21);
        let exit_player = PlayerId::from(22);

        let mut lanes = HashMap::new();
        lanes.insert(
            autosave_player,
            PlayerPersistenceLane {
                pending_save: Some(PendingSave {
                    snapshot: player_snapshot(autosave_player, 1.0, 2.0),
                    urgency: SaveUrgency::Autosave,
                    reply: None,
                }),
                ..Default::default()
            },
        );
        lanes.insert(
            flush_player,
            PlayerPersistenceLane {
                pending_save: Some(PendingSave {
                    snapshot: player_snapshot(flush_player, 3.0, 4.0),
                    urgency: SaveUrgency::FlushNow,
                    reply: Some(oneshot::channel().0),
                }),
                ..Default::default()
            },
        );
        lanes.insert(
            exit_player,
            PlayerPersistenceLane {
                pending_exit: Some(PendingExit {
                    exit_kind: PlayerExitKind::Disconnect,
                    username: "bob".to_string(),
                    server_id: "ch1-core".to_string(),
                    connection_id: "conn-2".to_string(),
                    snapshot: exit_snapshot(exit_player, 5.0, 6.0),
                    reply: oneshot::channel().0,
                }),
                ..Default::default()
            },
        );

        let ready = ready_player_ids(&lanes, now);
        assert_eq!(ready, vec![exit_player, flush_player, autosave_player]);
    }
}
