//! Typed database errors for the zohar-db crate.

use std::num::TryFromIntError;
#[cfg(feature = "db-game")]
use std::str::FromStr;

use thiserror::Error;

/// Result type used by public zohar-db APIs.
pub type DbResult<T> = Result<T, DbError>;

/// Database error type exposed by this crate.
#[derive(Debug, Error)]
pub enum DbError {
    /// Backend-specific error from sqlx.
    #[cfg(feature = "postgres-backend")]
    #[error("{context}: {source}")]
    Sqlx {
        context: String,
        #[source]
        source: sqlx::Error,
    },

    /// Backend-specific migration error from sqlx.
    #[cfg(feature = "postgres-backend")]
    #[error("{context}: {source}")]
    SqlxMigrate {
        context: String,
        #[source]
        source: sqlx::migrate::MigrateError,
    },

    /// Error while converting integer types.
    #[error("{context}: {source}")]
    IntConversion {
        context: String,
        #[source]
        source: TryFromIntError,
    },

    /// Failed to parse an enum value from the database.
    #[cfg(feature = "db-game")]
    #[error("parse {kind} from '{value}': {source}")]
    ParseEnum {
        kind: &'static str,
        value: String,
        #[source]
        source: strum::ParseError,
    },

    /// A database invariant was violated.
    #[error("database invariant violated: {0}")]
    Invariant(&'static str),

    /// Migration failed while applying or recording a change.
    #[cfg(feature = "postgres-backend")]
    #[error("migration '{name}' failed during {action}: {source}")]
    Migration {
        name: &'static str,
        action: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

/// Helper for attaching database context without `anyhow`.
pub trait DbContext<T> {
    fn db_ctx<C: Into<String>>(self, context: C) -> DbResult<T>;
}

#[cfg(feature = "postgres-backend")]
impl<T> DbContext<T> for Result<T, sqlx::Error> {
    fn db_ctx<C: Into<String>>(self, context: C) -> DbResult<T> {
        self.map_err(|source| DbError::Sqlx {
            context: context.into(),
            source,
        })
    }
}

#[cfg(feature = "postgres-backend")]
impl<T> DbContext<T> for Result<T, sqlx::migrate::MigrateError> {
    fn db_ctx<C: Into<String>>(self, context: C) -> DbResult<T> {
        self.map_err(|source| DbError::SqlxMigrate {
            context: context.into(),
            source,
        })
    }
}

impl<T> DbContext<T> for Result<T, TryFromIntError> {
    fn db_ctx<C: Into<String>>(self, context: C) -> DbResult<T> {
        self.map_err(|source| DbError::IntConversion {
            context: context.into(),
            source,
        })
    }
}

/// Helper for turning `Option<T>` into a typed invariant error.
pub trait OptionDbExt<T> {
    fn db_invariant(self, message: &'static str) -> DbResult<T>;
}

impl<T> OptionDbExt<T> for Option<T> {
    fn db_invariant(self, message: &'static str) -> DbResult<T> {
        self.ok_or(DbError::Invariant(message))
    }
}

/// Parse a string into an enum with a typed error.
#[cfg(feature = "db-game")]
pub fn parse_enum<T>(kind: &'static str, value: &str) -> DbResult<T>
where
    T: FromStr<Err = strum::ParseError>,
{
    T::from_str(value).map_err(|source| DbError::ParseEnum {
        kind,
        value: value.to_owned(),
        source,
    })
}

#[cfg(feature = "postgres-backend")]
impl DbError {
    /// Build a migration error with consistent formatting.
    pub fn migration(name: &'static str, action: &'static str, source: sqlx::Error) -> Self {
        Self::Migration {
            name,
            action,
            source,
        }
    }
}
