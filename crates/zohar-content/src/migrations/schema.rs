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

        let columns: Vec<String> = sqlx::query("PRAGMA table_info(mob_proto)")
            .map(|row: sqlx::sqlite::SqliteRow| row.get::<String, _>("name"))
            .fetch_all(&pool)
            .await
            .expect("mob proto columns");
        assert!(columns.iter().any(|column| column == "ai_flags"));
        assert!(columns.iter().any(|column| column == "battle_type"));
        assert!(columns.iter().any(|column| column == "aggressive_sight"));
        assert!(columns.iter().any(|column| column == "attack_range"));
    }

    #[tokio::test]
    async fn motion_set_mob_allows_multiple_mobs_per_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query("INSERT INTO enum_mob_type (value) VALUES ('MONSTER')")
            .execute(&pool)
            .await
            .expect("mob type");
        sqlx::query("INSERT INTO enum_mob_rank (value) VALUES ('PAWN')")
            .execute(&pool)
            .await
            .expect("mob rank");
        sqlx::query("INSERT INTO enum_battle_type (value) VALUES ('MELEE')")
            .execute(&pool)
            .await
            .expect("battle type");
        sqlx::query("INSERT INTO enum_motion_set_kind (value) VALUES ('MOB')")
            .execute(&pool)
            .await
            .expect("set kind");
        sqlx::query(
            "INSERT INTO mob_proto (mob_id, code, name, mob_type, rank, level)
             VALUES (101, 'MOB_101', 'Wolf A', 'MONSTER', 'PAWN', 1),
                    (102, 'MOB_102', 'Wolf B', 'MONSTER', 'PAWN', 1)",
        )
        .execute(&pool)
        .await
        .expect("mobs");
        sqlx::query("INSERT INTO motion_set (motion_set_id, set_kind) VALUES (200001, 'MOB')")
            .execute(&pool)
            .await
            .expect("motion set");
        sqlx::query(
            "INSERT INTO motion_set_mob (motion_set_id, mob_id)
             VALUES (200001, 101),
                    (200001, 102)",
        )
        .execute(&pool)
        .await
        .expect("motion links");

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM motion_set_mob WHERE motion_set_id = 200001")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn mob_proto_battle_type_requires_enum_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query("INSERT INTO enum_mob_type (value) VALUES ('MONSTER')")
            .execute(&pool)
            .await
            .expect("mob type");
        sqlx::query("INSERT INTO enum_mob_rank (value) VALUES ('PAWN')")
            .execute(&pool)
            .await
            .expect("mob rank");

        let result = sqlx::query(
            "INSERT INTO mob_proto (mob_id, code, name, mob_type, rank, battle_type, level)
             VALUES (101, 'MOB_101', 'Wolf A', 'MONSTER', 'PAWN', 'NOT_REAL', 1)",
        )
        .execute(&pool)
        .await;

        assert!(result.is_err(), "battle_type should be FK-validated");
    }

    #[tokio::test]
    async fn motion_set_player_allows_multiple_profiles_per_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query("INSERT INTO enum_player_class (value) VALUES ('WARRIOR'), ('NINJA')")
            .execute(&pool)
            .await
            .expect("player classes");
        sqlx::query("INSERT INTO enum_gender (value) VALUES ('MALE'), ('FEMALE')")
            .execute(&pool)
            .await
            .expect("genders");
        sqlx::query("INSERT INTO enum_motion_set_kind (value) VALUES ('PLAYER')")
            .execute(&pool)
            .await
            .expect("set kind");
        sqlx::query(
            "INSERT INTO player_motion_profile (profile_id, legacy_race_num, player_class, gender)
             VALUES (1, 0, 'WARRIOR', 'MALE'),
                    (2, 4, 'WARRIOR', 'FEMALE')",
        )
        .execute(&pool)
        .await
        .expect("profiles");
        sqlx::query("INSERT INTO motion_set (motion_set_id, set_kind) VALUES (300001, 'PLAYER')")
            .execute(&pool)
            .await
            .expect("motion set");
        sqlx::query(
            "INSERT INTO motion_set_player_profile (motion_set_id, profile_id)
             VALUES (300001, 1),
                    (300001, 2)",
        )
        .execute(&pool)
        .await
        .expect("player motion links");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM motion_set_player_profile WHERE motion_set_id = 300001",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn map_terrain_flags_trigger_rejects_mismatched_raw_len() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query(
            "INSERT INTO map_def (map_id, code, name, map_width, map_height)
             VALUES (1, 'map_a', 'Map A', 1024.0, 1280.0)",
        )
        .execute(&pool)
        .await
        .expect("map def");

        let err = sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.5, 'NONE', 3, X'010203')",
        )
        .execute(&pool)
        .await
        .expect_err("must reject mismatched raw_len");

        assert!(
            err.to_string()
                .contains("map_terrain_flags dimensions/raw_len mismatch"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn map_terrain_flags_trigger_rejects_non_integral_cell_derivation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query(
            "INSERT INTO map_def (map_id, code, name, map_width, map_height)
             VALUES (1, 'map_a', 'Map A', 1024.0, 1280.0)",
        )
        .execute(&pool)
        .await
        .expect("map def");

        let err = sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.3, 'NONE', 4, X'01020304')",
        )
        .execute(&pool)
        .await
        .expect_err("must reject non-integral derivation");

        assert!(
            err.to_string()
                .contains("map_terrain_flags dimensions/raw_len mismatch"),
            "unexpected error: {err}"
        );
    }
}
