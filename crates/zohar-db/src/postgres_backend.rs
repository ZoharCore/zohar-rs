//! PostgreSQL backend implementation.

mod bundles;
pub mod migrations;
pub mod queries;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::{DbContext, DbResult};
#[cfg(feature = "db-auth")]
pub use bundles::PgAuthDb;
#[cfg(feature = "db-game")]
pub use bundles::PgGameDb;

#[cfg(feature = "db-auth")]
pub async fn open_auth_db(database_url: &str) -> DbResult<PgAuthDb> {
    let pool = open_pool(database_url).await?;
    migrations::auth::run(&pool).await?;
    Ok(PgAuthDb::new(pool))
}

#[cfg(feature = "db-game")]
pub async fn open_game_db(database_url: &str) -> DbResult<PgGameDb> {
    let pool = open_pool(database_url).await?;
    migrations::game::run(&pool).await?;
    Ok(PgGameDb::new(pool))
}

#[cfg(feature = "db-game")]
pub async fn open_game_db_no_migrations(database_url: &str) -> DbResult<PgGameDb> {
    let pool = open_pool(database_url).await?;
    Ok(PgGameDb::new(pool))
}

#[cfg(all(feature = "db-auth", feature = "db-game"))]
pub async fn open_combined_db(database_url: &str) -> DbResult<(PgAuthDb, PgGameDb)> {
    let pool = open_pool(database_url).await?;
    migrations::auth::run(&pool).await?;
    migrations::game::run(&pool).await?;
    Ok((PgAuthDb::new(pool.clone()), PgGameDb::new(pool)))
}

async fn open_pool(database_url: &str) -> DbResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(16)
        .connect(database_url)
        .await
        .db_ctx(format!("connect postgres database at {database_url}"))
}

#[cfg(all(test, feature = "db-auth", feature = "db-game"))]
mod tests {
    use super::*;
    use crate::traits::{
        AccountsView, AcquireSessionResult, AuthDb, GameDb, ProfilesView, SessionsView,
    };
    use sqlx::Row;

    fn test_db_url() -> Option<String> {
        std::env::var("ZOHAR_TEST_DATABASE_URL").ok()
    }

    #[tokio::test]
    async fn test_open_combined_db() -> DbResult<()> {
        let Some(db_url) = test_db_url() else {
            return Ok(());
        };

        let (auth_db, game_db) = open_combined_db(&db_url).await?;

        let account = auth_db.accounts().find_by_username("admin").await?;
        assert!(account.is_some());

        let profile = game_db.profiles().get_or_create("admin").await?;
        assert_eq!(profile.username, "admin");

        Ok(())
    }

    #[tokio::test]
    async fn test_session_acquire() -> DbResult<()> {
        let Some(db_url) = test_db_url() else {
            return Ok(());
        };

        let (_auth_db, game_db) = open_combined_db(&db_url).await?;

        let result = game_db
            .sessions()
            .acquire("admin", "server1", "conn1", 60)
            .await?;
        assert!(matches!(result, AcquireSessionResult::Acquired));

        let result = game_db
            .sessions()
            .acquire("admin", "server2", "conn2", 60)
            .await?;
        assert!(matches!(
            result,
            AcquireSessionResult::AlreadyOnOtherServer { .. }
        ));

        Ok(())
    }

    #[tokio::test]
    async fn test_session_timestamp_storage_types() -> DbResult<()> {
        let Some(db_url) = test_db_url() else {
            return Ok(());
        };

        let (_auth_db, game_db) = open_combined_db(&db_url).await?;

        let last_heartbeat_type = sqlx::query(
            "SELECT pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_name
             FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON a.attrelid = c.oid
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
             WHERE n.nspname = 'game'
               AND c.relname = 'sessions'
               AND a.attname = 'last_heartbeat'
               AND a.attnum > 0
               AND NOT a.attisdropped",
        )
        .fetch_one(game_db.pool())
        .await
        .db_ctx("query game.sessions.last_heartbeat type")?
        .try_get::<String, _>("type_name")
        .db_ctx("read game.sessions.last_heartbeat type")?;
        assert_eq!(last_heartbeat_type, "timestamp with time zone");

        let login_issued_at_type = sqlx::query(
            "SELECT pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_name
             FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON a.attrelid = c.oid
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
             WHERE n.nspname = 'game'
               AND c.relname = 'sessions'
               AND a.attname = 'login_issued_at'
               AND a.attnum > 0
               AND NOT a.attisdropped",
        )
        .fetch_one(game_db.pool())
        .await
        .db_ctx("query game.sessions.login_issued_at type")?
        .try_get::<String, _>("type_name")
        .db_ctx("read game.sessions.login_issued_at type")?;
        assert_eq!(login_issued_at_type, "timestamp with time zone");

        Ok(())
    }
}
