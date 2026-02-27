//! Postgres migrations for auth/game schemas.

use sqlx::PgPool;
use sqlx::migrate::Migrator;
use tracing::info;

use crate::{DbContext, DbResult};

static DB_MIGRATOR: Migrator = sqlx::migrate!("./src/postgres_backend/migrations");

#[cfg(feature = "db-auth")]
pub mod auth {
    use super::*;

    pub async fn run(pool: &PgPool) -> DbResult<()> {
        info!("Checking auth database migrations...");
        run_migrations(pool).await?;
        info!("Auth database is up to date.");
        Ok(())
    }
}

#[cfg(feature = "db-game")]
pub mod game {
    use super::*;

    pub async fn run(pool: &PgPool) -> DbResult<()> {
        info!("Checking game database migrations...");
        run_migrations(pool).await?;
        info!("Game database is up to date.");
        Ok(())
    }
}

async fn run_migrations(pool: &PgPool) -> DbResult<()> {
    DB_MIGRATOR
        .run(pool)
        .await
        .db_ctx("run sqlx postgres migrations")
}
