pub(crate) type PhaseResult<T> = Result<T, anyhow::Error>;

#[derive(Debug)]
pub(crate) enum SessionEnd {
    BeforeLogin,
    AfterLogin { username: String },
}
