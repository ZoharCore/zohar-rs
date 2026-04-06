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
