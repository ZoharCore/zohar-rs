use std::time::Duration;

use zohar_map_port::{ClientTimestamp, PacketDuration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub(crate) struct SimInstant(u64);

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

    pub(crate) fn to_client_timestamp(self) -> ClientTimestamp {
        ClientTimestamp::new(self.0.min(u64::from(u32::MAX)) as u32)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_instant_clamps_to_packet_timestamp() {
        let packet_ts = SimInstant::from_millis(u64::from(u32::MAX) + 42).to_client_timestamp();
        assert_eq!(packet_ts.get(), u32::MAX);
    }

    #[test]
    fn sim_time_arithmetic_saturates() {
        let start = SimInstant::from_millis(u64::MAX - 4);
        assert_eq!(
            u64::from(start.saturating_add(SimDuration::from_millis(10))),
            u64::MAX
        );
        assert_eq!(
            u64::from(start.saturating_sub(SimInstant::from_millis(10))),
            u64::MAX - 14
        );
    }

    #[test]
    fn transparent_scalars_keep_primitive_size() {
        assert_eq!(
            std::mem::size_of::<SimInstant>(),
            std::mem::size_of::<u64>()
        );
        assert_eq!(
            std::mem::size_of::<SimDuration>(),
            std::mem::size_of::<u64>()
        );
    }
}
