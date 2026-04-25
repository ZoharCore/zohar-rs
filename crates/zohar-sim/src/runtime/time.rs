use std::time::Duration;

use zohar_map_port::{ClientTimestamp, PacketDuration};

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub(crate) struct SimInstant(u64);

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub(crate) struct SimDuration(u64);

impl SimInstant {
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn from_elapsed(elapsed: Duration) -> Self {
        Self::from_millis(elapsed.as_millis().min(u64::MAX as u128) as u64)
    }

    pub(crate) const fn saturating_add(self, duration: SimDuration) -> Self {
        Self(self.0.saturating_add(duration.0))
    }

    pub(crate) const fn saturating_sub(self, earlier: Self) -> SimDuration {
        SimDuration(self.0.saturating_sub(earlier.0))
    }

    pub(crate) const fn overflowing_sub(self, earlier: Self) -> (SimDuration, bool) {
        let (wrapped, did_underflow) = self.0.overflowing_sub(earlier.0);
        (SimDuration(wrapped), did_underflow)
    }

    pub(crate) fn to_client_timestamp(self) -> ClientTimestamp {
        ClientTimestamp::new(self.0.min(u64::from(u32::MAX)) as u32)
    }

    pub(crate) fn elapsed_since(self, event: Option<SimInstant>) -> Option<Duration> {
        let (sim_duration, did_underflow) = self.overflowing_sub(event?);
        if did_underflow {
            None
        } else {
            Some(sim_duration.as_duration())
        }
    }
}

impl From<u64> for SimInstant {
    fn from(value: u64) -> Self {
        Self::from_millis(value)
    }
}

impl From<SimInstant> for u64 {
    fn from(value: SimInstant) -> Self {
        value.0
    }
}

impl SimDuration {
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn from_packet_duration(value: PacketDuration) -> Self {
        Self(u64::from(value.get()))
    }

    pub(crate) const fn as_millis(self) -> u64 {
        self.0
    }

    pub(crate) fn as_duration(self) -> Duration {
        Duration::from_millis(self.0)
    }
}

impl From<u64> for SimDuration {
    fn from(value: u64) -> Self {
        Self::from_millis(value)
    }
}

impl From<SimDuration> for u64 {
    fn from(value: SimDuration) -> Self {
        value.0
    }
}

impl From<Duration> for SimDuration {
    fn from(value: Duration) -> Self {
        Self::from_millis(value.as_millis().min(u64::MAX as u128) as u64)
    }
}

impl From<SimDuration> for Duration {
    fn from(value: SimDuration) -> Self {
        value.as_duration()
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SimTickerClock {
    last_processed_at: SimInstant,
    next_due_at: SimInstant,
    cadence: SimDuration,
}

impl SimTickerClock {
    pub(crate) fn phased(seed: i64, now: SimInstant, cadence: SimDuration) -> Self {
        assert!(
            cadence > SimDuration::ZERO,
            "ticker cadence must be non-zero"
        );

        let cadence_ms = cadence.as_millis();
        let phase_ms = seed.unsigned_abs() % cadence_ms;
        let now_phase = u64::from(now) % cadence_ms;
        let delay_ms = if phase_ms > now_phase {
            phase_ms - now_phase
        } else {
            cadence_ms - (now_phase - phase_ms)
        };

        Self::scheduled(
            now,
            now.saturating_add(SimDuration::from_millis(delay_ms.max(1))),
            cadence,
        )
    }

    pub(crate) fn scheduled(
        last_processed_at: SimInstant,
        next_due_at: SimInstant,
        cadence: SimDuration,
    ) -> Self {
        assert!(
            cadence > SimDuration::ZERO,
            "ticker cadence must be non-zero"
        );

        Self {
            last_processed_at,
            next_due_at,
            cadence,
        }
    }

    #[cfg(test)]
    pub(crate) fn next_due_at(&self) -> SimInstant {
        self.next_due_at
    }

    pub(crate) fn is_due(&self, now: SimInstant) -> bool {
        self.next_due_at <= now
    }

    pub(crate) fn advance_due(&mut self, now: SimInstant) -> Option<SimDuration> {
        if !self.is_due(now) {
            return None;
        }

        let elapsed = now.saturating_sub(self.last_processed_at);
        self.last_processed_at = now;
        self.advance_past(now);
        Some(elapsed)
    }

    pub(crate) fn retry_after(&mut self, now: SimInstant, delay: SimDuration) {
        self.last_processed_at = now;
        self.next_due_at = now.saturating_add(delay);
    }

    fn advance_past(&mut self, now: SimInstant) {
        while self.next_due_at <= now {
            self.next_due_at = self.next_due_at.saturating_add(self.cadence);
        }
    }
}
