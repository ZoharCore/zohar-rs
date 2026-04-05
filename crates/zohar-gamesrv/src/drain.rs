use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::watch;

#[derive(Clone)]
pub struct ServerDrainController {
    inner: Arc<Inner>,
}

struct Inner {
    draining: AtomicBool,
    active_connections: AtomicUsize,
    drain_tx: watch::Sender<bool>,
}

impl ServerDrainController {
    pub fn new() -> Self {
        let (drain_tx, _drain_rx) = watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                draining: AtomicBool::new(false),
                active_connections: AtomicUsize::new(0),
                drain_tx,
            }),
        }
    }

    pub fn begin_draining(&self) -> bool {
        if self.inner.draining.swap(true, Ordering::SeqCst) {
            return false;
        }
        let _ = self.inner.drain_tx.send(true);
        true
    }

    pub fn is_draining(&self) -> bool {
        self.inner.draining.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.inner.drain_tx.subscribe()
    }

    pub fn track_connection(&self) -> ServerConnectionGuard {
        self.inner.active_connections.fetch_add(1, Ordering::SeqCst);
        ServerConnectionGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn active_connections(&self) -> usize {
        self.inner.active_connections.load(Ordering::SeqCst)
    }
}

impl Default for ServerDrainController {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ServerConnectionGuard {
    inner: Arc<Inner>,
}

impl Drop for ServerConnectionGuard {
    fn drop(&mut self) {
        self.inner.active_connections.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::ServerDrainController;

    #[test]
    fn connection_guard_tracks_active_connection_count() {
        let drain = ServerDrainController::new();
        let guard_a = drain.track_connection();
        let guard_b = drain.track_connection();
        assert_eq!(drain.active_connections(), 2);
        drop(guard_a);
        assert_eq!(drain.active_connections(), 1);
        drop(guard_b);
        assert_eq!(drain.active_connections(), 0);
    }

    #[tokio::test]
    async fn begin_draining_notifies_subscribers() {
        let drain = ServerDrainController::new();
        let mut rx = drain.subscribe();
        assert!(drain.begin_draining());
        rx.changed().await.expect("drain notification");
        assert!(*rx.borrow());
        assert!(drain.is_draining());
        assert!(!drain.begin_draining());
    }
}
