use std::fmt;

use num_enum::{IntoPrimitive, TryFromPrimitive};

#[binrw::binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum PhaseId {
    Handshake = 1,
    Login = 2,
    Select = 3,
    Loading = 4,
    InGame = 5,
    Auth = 10,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PhaseMismatchError {
    actual: PhaseId,
}

impl PhaseMismatchError {
    pub fn new(actual: PhaseId) -> Self {
        Self { actual }
    }

    pub fn actual(&self) -> PhaseId {
        self.actual
    }
}

impl fmt::Display for PhaseMismatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Phase mismatch: connection was on {:?}", self.actual)
    }
}

impl std::error::Error for PhaseMismatchError {}
