use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct HandshakeId(u32);

impl HandshakeId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for HandshakeId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<HandshakeId> for u32 {
    fn from(value: HandshakeId) -> Self {
        value.0
    }
}

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct WireMillis32(u32);

impl WireMillis32 {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.0 as u64)
    }
}

impl From<u32> for WireMillis32 {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<WireMillis32> for u32 {
    fn from(value: WireMillis32) -> Self {
        value.0
    }
}

impl From<WireMillis32> for Duration {
    fn from(value: WireMillis32) -> Self {
        value.as_duration()
    }
}

impl From<Duration> for WireMillis32 {
    fn from(value: Duration) -> Self {
        Self(value.as_millis() as u32)
    }
}

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct WireDeltaMillis {
    #[br(assert(value >= 0, "negative handshake delta"))]
    #[bw(assert(*value >= 0, "negative handshake delta"))]
    value: i32,
}

impl WireDeltaMillis {
    pub const fn get(self) -> i32 {
        self.value
    }

    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.value as u64)
    }
}

impl From<WireDeltaMillis> for Duration {
    fn from(value: WireDeltaMillis) -> Self {
        value.as_duration()
    }
}

impl From<WireDeltaMillis> for i32 {
    fn from(value: WireDeltaMillis) -> Self {
        value.value
    }
}

impl From<Duration> for WireDeltaMillis {
    fn from(value: Duration) -> Self {
        let millis = value.as_millis();
        let capped = if millis > i32::MAX as u128 {
            i32::MAX
        } else {
            millis as i32
        };
        Self { value: capped }
    }
}

#[binrw::binrw]
#[br(little)]
#[bw(little)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct HandshakeSyncData {
    pub handshake: HandshakeId,
    pub time: WireMillis32,
    pub delta: WireDeltaMillis,
}

const HANDSHAKE_RETRY_LIMIT: u8 = 32;
const HANDSHAKE_BIAS_TOLERANCE_MS: i64 = 50;

#[derive(Debug)]
pub struct HandshakeState {
    id: HandshakeId,
    server_start: Instant,
    last_sent: Instant,
    retries: u8,
    handshaking: bool,
}

#[derive(Debug)]
pub enum HandshakeOutcome {
    CompletedInitial,
    SendHandshakeSync(HandshakeSyncData),
    SendTimeSyncAck,
}

#[derive(Debug)]
pub enum HandshakeError {
    HandshakeMismatch,
    RetryLimitExceeded,
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::HandshakeMismatch => write!(f, "handshake id mismatch"),
            HandshakeError::RetryLimitExceeded => write!(f, "handshake retry limit exceeded"),
        }
    }
}

impl std::error::Error for HandshakeError {}

impl HandshakeState {
    pub fn new(server_start: Instant, now: Instant) -> Self {
        Self::with_seed(server_start, now, seed_u32())
    }

    pub fn with_seed(server_start: Instant, now: Instant, seed: u32) -> Self {
        let id = HandshakeId::new(seed);
        Self {
            id,
            server_start,
            last_sent: now,
            retries: 0,
            handshaking: true,
        }
    }

    pub fn is_handshaking(&self) -> bool {
        self.handshaking
    }

    pub fn initial_sync_data(&mut self, now: Instant) -> HandshakeSyncData {
        self.sync_data(now, Duration::ZERO)
    }

    pub fn sync_data(&mut self, now: Instant, delta: Duration) -> HandshakeSyncData {
        let data = self.build_sync_data(now, delta);
        self.last_sent = now;
        data
    }

    pub fn handle(
        &mut self,
        data: HandshakeSyncData,
        now: Instant,
    ) -> Result<HandshakeOutcome, HandshakeError> {
        if data.handshake != self.id {
            return Err(HandshakeError::HandshakeMismatch);
        }

        let server_uptime = self.uptime_at(now);
        let client_time = data.time.as_duration();
        let client_delta = data.delta.as_duration();
        let bias_ms = millis_i64(server_uptime) - millis_i64(client_time + client_delta);

        if (0..=HANDSHAKE_BIAS_TOLERANCE_MS).contains(&bias_ms) {
            if self.handshaking {
                self.handshaking = false;
                self.retries = 0;
                return Ok(HandshakeOutcome::CompletedInitial);
            }
            return Ok(HandshakeOutcome::SendTimeSyncAck);
        }

        let mut delta_ms = (millis_i64(server_uptime) - millis_i64(client_time)) / 2;
        if delta_ms < 0 {
            let last_sent_ms = millis_i64(self.uptime_at(self.last_sent));
            delta_ms = (millis_i64(server_uptime) - last_sent_ms) / 2;
        }
        if delta_ms < 0 {
            delta_ms = 0;
        }

        let delta_duration = Duration::from_millis(delta_ms as u64);
        let data = self.build_sync_data(now, delta_duration);

        if self.handshaking {
            let next_retry = self.retries.saturating_add(1);
            if next_retry > HANDSHAKE_RETRY_LIMIT {
                return Err(HandshakeError::RetryLimitExceeded);
            }
            self.retries = next_retry;
        }
        self.last_sent = now;

        Ok(HandshakeOutcome::SendHandshakeSync(data))
    }

    fn build_sync_data(&self, now: Instant, delta: Duration) -> HandshakeSyncData {
        HandshakeSyncData {
            handshake: self.id,
            time: WireMillis32::from(self.uptime_at(now)),
            delta: WireDeltaMillis::from(delta),
        }
    }

    pub fn uptime_at(&self, now: Instant) -> Duration {
        now.duration_since(self.server_start)
    }
}

fn millis_i64(duration: Duration) -> i64 {
    let millis = duration.as_millis();
    if millis > i64::MAX as u128 {
        i64::MAX
    } else {
        millis as i64
    }
}

fn seed_u32() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mixed = (nanos as u64) ^ ((nanos >> 64) as u64);
    (mixed ^ (mixed >> 32)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_phase_time_sync_with_acceptable_bias_acks_directly() {
        let start = Instant::now();
        let mut hs = HandshakeState::with_seed(start, start, 7);

        let t0 = start + Duration::from_millis(1_000);
        let initial = hs.initial_sync_data(t0);
        let initial_client_reply = HandshakeSyncData {
            handshake: initial.handshake,
            time: WireMillis32::from(Duration::from_millis(1_000)),
            delta: WireDeltaMillis::from(Duration::ZERO),
        };
        let outcome = hs
            .handle(initial_client_reply, t0)
            .expect("initial handshake");
        assert!(matches!(outcome, HandshakeOutcome::CompletedInitial));

        let t1 = start + Duration::from_millis(2_000);
        let post_phase_reply_1 = HandshakeSyncData {
            handshake: initial.handshake,
            time: WireMillis32::from(Duration::from_millis(2_000)),
            delta: WireDeltaMillis::from(Duration::ZERO),
        };
        let outcome = hs.handle(post_phase_reply_1, t1).expect("post-phase sync");
        assert!(
            matches!(outcome, HandshakeOutcome::SendTimeSyncAck),
            "post-phase sync should use ack-only path when bias is acceptable"
        );
    }
}
