use std::path::Path;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::error::ContentError;

pub async fn open_fresh_connection(path: &Path) -> Result<SqlitePool, ContentError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    remove_if_exists(path)?;
    remove_if_exists(&sqlite_sidecar_path(path, "wal"))?;
    remove_if_exists(&sqlite_sidecar_path(path, "shm"))?;

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(ContentError::from)
}

fn remove_if_exists(path: &Path) -> Result<(), ContentError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}-{suffix}", path.display()))
}

pub async fn open_existing_read_only(path: &Path) -> Result<SqlitePool, ContentError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .foreign_keys(true);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(ContentError::from)
}
