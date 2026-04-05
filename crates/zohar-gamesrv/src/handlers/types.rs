pub(crate) type PhaseResult<T> = Result<T, anyhow::Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionLeaseAction {
    Release,
    AlreadyReleased,
    RetainUntilStale,
}

#[derive(Debug)]
pub(crate) enum SessionEnd {
    BeforeLogin,
    AfterLogin {
        username: String,
        lease_action: SessionLeaseAction,
    },
}
