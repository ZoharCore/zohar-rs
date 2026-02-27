use std::time::{Duration, Instant};

const HEARTBEAT_TIMEOUT_MULTIPLIER: u32 = 3;

pub(crate) enum SessionTick {
    SendHeartbeat,
    TimedOut,
}

pub(crate) struct SessionTracker {
    last_rx: Instant,
    heartbeat_interval: Duration,
    timeout: Duration,
}

impl SessionTracker {
    pub fn new(now: Instant, heartbeat_interval: Duration) -> Self {
        let timeout = heartbeat_interval
            .checked_mul(HEARTBEAT_TIMEOUT_MULTIPLIER)
            .unwrap_or(Duration::from_secs(u64::MAX));
        Self {
            last_rx: now,
            heartbeat_interval,
            timeout,
        }
    }

    pub fn mark_rx(&mut self, now: Instant) {
        self.last_rx = now;
    }

    pub fn on_tick(&self, now: Instant) -> Option<SessionTick> {
        let since_rx = now.saturating_duration_since(self.last_rx);
        if since_rx >= self.timeout {
            return Some(SessionTick::TimedOut);
        }
        if since_rx >= self.heartbeat_interval {
            return Some(SessionTick::SendHeartbeat);
        }
        None
    }
}
