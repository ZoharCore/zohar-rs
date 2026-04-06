use std::fmt;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct Facing72(u8);

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facing72Error {
    value: u8,
}

impl Facing72Error {
    pub const fn raw(self) -> u8 {
        self.value
    }
}

impl fmt::Display for Facing72Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "facing {} is out of range 0..=71", self.value)
    }
}

impl std::error::Error for Facing72Error {}

impl Facing72 {
    pub const MIN: u8 = 0;
    pub const MAX: u8 = 71;

    pub fn new(value: u8) -> Result<Self, Facing72Error> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(Facing72Error { value })
        }
    }

    pub const fn from_wrapped(value: u8) -> Self {
        Self(value % 72)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Facing72 {
    type Error = Facing72Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Facing72> for u8 {
    fn from(value: Facing72) -> Self {
        value.0
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct ClientTimestamp(u32);

impl ClientTimestamp {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn saturating_add(self, duration: PacketDuration) -> Self {
        Self(self.0.saturating_add(duration.0))
    }

    pub const fn saturating_sub(self, earlier: Self) -> PacketDuration {
        PacketDuration(self.0.saturating_sub(earlier.0))
    }
}

impl From<u32> for ClientTimestamp {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ClientTimestamp> for u32 {
    fn from(value: ClientTimestamp) -> Self {
        value.0
    }
}

impl From<ClientTimestamp> for u64 {
    fn from(value: ClientTimestamp) -> Self {
        u64::from(value.0)
    }
}

impl PartialEq<u32> for ClientTimestamp {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u32> for ClientTimestamp {
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PacketDuration(u32);

impl PacketDuration {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for PacketDuration {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<PacketDuration> for u32 {
    fn from(value: PacketDuration) -> Self {
        value.0
    }
}

impl From<PacketDuration> for u64 {
    fn from(value: PacketDuration) -> Self {
        u64::from(value.0)
    }
}

impl PartialEq<u32> for PacketDuration {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u32> for PacketDuration {
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl std::ops::Div<u32> for PacketDuration {
    type Output = Self;

    fn div(self, rhs: u32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct MovementArg(u8);

impl MovementArg {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn basic_attack() -> Self {
        Self::ZERO
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<u8> for MovementArg {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<MovementArg> for u8 {
    fn from(value: MovementArg) -> Self {
        value.0
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatChannel {
    Speak,
    Info,
    Notice,
    Command,
    Shout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing72_rejects_out_of_range_values() {
        assert_eq!(Facing72::try_from(71).expect("valid").get(), 71);
        assert_eq!(Facing72::try_from(72).expect_err("invalid").raw(), 72);
    }
}
