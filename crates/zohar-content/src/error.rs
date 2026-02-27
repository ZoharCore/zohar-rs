use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] sqlx::Error),

    #[error("invalid migration filename: {0}")]
    InvalidMigrationName(String),

    #[error("duplicate numeric prefix '{prefix}' in {scope}")]
    DuplicatePrefix { scope: String, prefix: String },

    #[error("hash drift for migration '{path}': expected {expected_hash}, found {actual_hash}")]
    MigrationHashDrift {
        path: String,
        expected_hash: String,
        actual_hash: String,
    },

    #[error("path is not valid unicode: {0:?}")]
    NonUtf8Path(PathBuf),

    #[error("invalid enum value '{value}' for {kind}")]
    InvalidEnum { kind: &'static str, value: String },
}

pub(crate) fn parse_enum<E>(raw: &str, kind: &'static str) -> Result<E, ContentError>
where
    E: std::str::FromStr,
{
    raw.parse::<E>().map_err(|_| ContentError::InvalidEnum {
        kind,
        value: raw.to_string(),
    })
}
