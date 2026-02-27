use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use crate::error::ContentError;
use crate::runtime::AppliedMigration;

struct SchemaMigration {
    version: i64,
    id: &'static str,
    sql: &'static str,
}

static SCHEMA_MIGRATIONS: &[SchemaMigration] = &[
    SchemaMigration {
        version: 1,
        id: "V0001__core.sql",
        sql: include_str!("schema/V0001__core.sql"),
    },
    SchemaMigration {
        version: 10,
        id: "V0010__enums.sql",
        sql: include_str!("schema/V0010__enums.sql"),
    },
    SchemaMigration {
        version: 20,
        id: "V0020__maps.sql",
        sql: include_str!("schema/V0020__maps.sql"),
    },
    SchemaMigration {
        version: 30,
        id: "V0030__mobs.sql",
        sql: include_str!("schema/V0030__mobs.sql"),
    },
    SchemaMigration {
        version: 40,
        id: "V0040__motion.sql",
        sql: include_str!("schema/V0040__motion.sql"),
    },
    SchemaMigration {
        version: 50,
        id: "V0050__spawns.sql",
        sql: include_str!("schema/V0050__spawns.sql"),
    },
];

pub async fn apply_schema_migrations(
    conn: &SqlitePool,
) -> Result<Vec<AppliedMigration>, ContentError> {
    let mut db = conn.acquire().await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _content_schema_migrations (
            version INTEGER PRIMARY KEY,
            id TEXT NOT NULL,
            hash TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&mut *db)
    .await?;

    let mut applied = Vec::new();
    for migration in SCHEMA_MIGRATIONS {
        let hash = hash_text(migration.sql);

        let existing_row =
            sqlx::query("SELECT id, hash FROM _content_schema_migrations WHERE version = ?1")
                .bind(migration.version)
                .fetch_optional(&mut *db)
                .await?;

        if let Some(row) = existing_row {
            let existing_id: String = row.try_get(0)?;
            let existing_hash: String = row.try_get(1)?;
            if existing_hash != hash {
                return Err(ContentError::MigrationHashDrift {
                    path: existing_id,
                    expected_hash: existing_hash,
                    actual_hash: hash,
                });
            }
            continue;
        }

        sqlx::query("BEGIN IMMEDIATE").execute(&mut *db).await?;
        if let Err(err) = sqlx::raw_sql(migration.sql).execute(&mut *db).await {
            let _ = sqlx::query("ROLLBACK").execute(&mut *db).await;
            return Err(err.into());
        }
        if let Err(err) = sqlx::query(
            "INSERT INTO _content_schema_migrations (version, id, hash) VALUES (?1, ?2, ?3)",
        )
        .bind(migration.version)
        .bind(migration.id)
        .bind(&hash)
        .execute(&mut *db)
        .await
        {
            let _ = sqlx::query("ROLLBACK").execute(&mut *db).await;
            return Err(err.into());
        }
        sqlx::query("COMMIT").execute(&mut *db).await?;

        applied.push(AppliedMigration {
            id: migration.id.to_string(),
            hash,
            rejected_count: 0,
        });
    }

    Ok(applied)
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_fresh_connection;

    #[tokio::test]
    async fn schema_migrations_are_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        let first = apply_schema_migrations(&pool).await.expect("first");
        assert!(!first.is_empty());

        let second = apply_schema_migrations(&pool).await.expect("second");
        assert!(second.is_empty());

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _content_schema_migrations")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, SCHEMA_MIGRATIONS.len() as i64);
    }
}
