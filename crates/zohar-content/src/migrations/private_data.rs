use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tracing::warn;

use crate::error::ContentError;
use crate::runtime::{AppliedMigration, RejectedStatement};

#[derive(Debug, Clone)]
struct DiscoveredMigration {
    id: String,
    hash: String,
    sql: String,
}

pub async fn apply_private_data_migrations(
    conn: &SqlitePool,
    root: &Path,
) -> Result<(Vec<AppliedMigration>, Vec<RejectedStatement>), ContentError> {
    let mut db = conn.acquire().await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _content_data_migrations (
            path TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            rejected_count INTEGER NOT NULL,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&mut *db)
    .await?;

    let migrations = discover_private_migrations(root)?;
    let mut applied = Vec::new();
    let mut rejected = Vec::new();

    for migration in migrations {
        let existing_hash =
            sqlx::query("SELECT hash FROM _content_data_migrations WHERE path = ?1")
                .bind(&migration.id)
                .fetch_optional(&mut *db)
                .await?
                .map(|row| row.try_get::<String, _>(0))
                .transpose()?;

        if let Some(existing_hash) = existing_hash {
            if existing_hash != migration.hash {
                return Err(ContentError::MigrationHashDrift {
                    path: migration.id,
                    expected_hash: existing_hash,
                    actual_hash: migration.hash,
                });
            }
            continue;
        }

        let mut migration_rejected = Vec::new();
        for (statement_index, statement) in
            split_sql_statements(&migration.sql).into_iter().enumerate()
        {
            let savepoint = format!("sp_{}_{}", sanitize_id(&migration.id), statement_index);
            sqlx::query(&format!("SAVEPOINT {savepoint}"))
                .execute(&mut *db)
                .await?;

            match sqlx::raw_sql(&statement).execute(&mut *db).await {
                Ok(_) => {
                    sqlx::query(&format!("RELEASE SAVEPOINT {savepoint}"))
                        .execute(&mut *db)
                        .await?;
                }
                Err(err) => {
                    sqlx::raw_sql(&format!(
                        "ROLLBACK TO SAVEPOINT {savepoint}; RELEASE SAVEPOINT {savepoint};"
                    ))
                    .execute(&mut *db)
                    .await?;
                    let rejection = RejectedStatement {
                        migration_id: migration.id.clone(),
                        statement_index,
                        error: err.to_string(),
                    };
                    warn!(
                        migration = %rejection.migration_id,
                        statement_index = rejection.statement_index,
                        error = %rejection.error,
                        "Rejected private content statement"
                    );
                    migration_rejected.push(rejection);
                }
            }
        }

        sqlx::query(
            "INSERT INTO _content_data_migrations (path, hash, rejected_count) VALUES (?1, ?2, ?3)",
        )
        .bind(&migration.id)
        .bind(&migration.hash)
        .bind(migration_rejected.len() as i64)
        .execute(&mut *db)
        .await?;

        applied.push(AppliedMigration {
            id: migration.id.clone(),
            hash: migration.hash,
            rejected_count: migration_rejected.len(),
        });
        rejected.extend(migration_rejected);
    }

    Ok((applied, rejected))
}

fn discover_private_migrations(root: &Path) -> Result<Vec<DiscoveredMigration>, ContentError> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut packs = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        packs.push(entry.path());
    }

    packs.sort();
    ensure_unique_numeric_prefixes(&packs, "data/content packs")?;

    let mut out = Vec::new();
    for pack_dir in packs {
        let pack_name = file_name_string(&pack_dir)?;

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&pack_dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
                continue;
            }
            files.push(path);
        }

        files.sort();
        ensure_unique_numeric_prefixes(&files, &format!("pack {pack_name}"))?;

        for file in files {
            let file_name = file_name_string(&file)?;
            let rel_id = format!("{pack_name}/{file_name}");
            let sql = std::fs::read_to_string(&file)?;
            let hash = hash_text(&sql);
            out.push(DiscoveredMigration {
                id: rel_id,
                hash,
                sql,
            });
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn file_name_string(path: &Path) -> Result<String, ContentError> {
    let file_name = path
        .file_name()
        .ok_or_else(|| ContentError::NonUtf8Path(path.to_path_buf()))?;
    let file_name = file_name
        .to_str()
        .ok_or_else(|| ContentError::NonUtf8Path(path.to_path_buf()))?;
    Ok(file_name.to_string())
}

fn ensure_unique_numeric_prefixes(paths: &[PathBuf], scope: &str) -> Result<(), ContentError> {
    let mut seen = HashSet::new();
    for path in paths {
        let file_name = file_name_string(path)?;
        let Some(prefix) = numeric_prefix(&file_name) else {
            continue;
        };
        if !seen.insert(prefix.clone()) {
            return Err(ContentError::DuplicatePrefix {
                scope: scope.to_string(),
                prefix,
            });
        }
    }
    Ok(())
}

fn numeric_prefix(name: &str) -> Option<String> {
    let digits: String = name.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

pub(crate) fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    let mut chars = sql.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                current.push(ch);
            }
            continue;
        }

        if in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        if !in_single_quote && !in_double_quote {
            if ch == '-' && chars.peek() == Some(&'-') {
                chars.next();
                in_line_comment = true;
                continue;
            }

            if ch == '/' && chars.peek() == Some(&'*') {
                chars.next();
                in_block_comment = true;
                continue;
            }
        }

        if ch == '\'' && !in_double_quote {
            if in_single_quote && chars.peek() == Some(&'\'') {
                current.push(ch);
                current.push(chars.next().unwrap_or('\''));
                continue;
            }
            in_single_quote = !in_single_quote;
            current.push(ch);
            continue;
        }

        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(ch);
            continue;
        }

        if ch == ';' && !in_single_quote && !in_double_quote {
            let stmt = current.trim();
            if !stmt.is_empty() {
                out.push(stmt.to_string());
            }
            current.clear();
            continue;
        }

        current.push(ch);
    }

    let trailing = current.trim();
    if !trailing.is_empty() {
        out.push(trailing.to_string());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_fresh_connection;

    #[test]
    fn split_statements_handles_quotes_comments() {
        let sql = "-- comment\nINSERT INTO t VALUES ('a;');\n/* block ; */\nINSERT INTO t VALUES (\"b\");";
        let parts = split_sql_statements(sql);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn migration_discovery_is_lexicographically_ordered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_a = dir.path().join("020_expansion");
        let pack_b = dir.path().join("010_base");
        std::fs::create_dir_all(&pack_a).expect("pack a");
        std::fs::create_dir_all(&pack_b).expect("pack b");

        std::fs::write(pack_a.join("0002__second.sql"), "SELECT 1;").expect("seed");
        std::fs::write(pack_b.join("0001__first.sql"), "SELECT 1;").expect("seed");

        let migrations = discover_private_migrations(dir.path()).expect("discover");
        let ids: Vec<String> = migrations.into_iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![
                "010_base/0001__first.sql".to_string(),
                "020_expansion/0002__second.sql".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn rejected_rows_are_logged_and_continue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("010_base");
        std::fs::create_dir_all(&pack).expect("pack");
        std::fs::write(
            pack.join("0001__seed.sql"),
            r#"
            CREATE TABLE parent (id INTEGER PRIMARY KEY);
            CREATE TABLE child (
                id INTEGER PRIMARY KEY,
                parent_id INTEGER NOT NULL REFERENCES parent(id)
            );
            INSERT INTO child (id, parent_id) VALUES (1, 999);
            INSERT INTO parent (id) VALUES (999);
            "#,
        )
        .expect("seed");

        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");

        let (applied, rejected) = apply_private_data_migrations(&pool, dir.path())
            .await
            .expect("apply");
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].rejected_count, 1);
        assert_eq!(rejected.len(), 1);

        let parent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM parent")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(parent_count, 1);
    }

    #[tokio::test]
    async fn drift_is_detected_for_same_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("010_base");
        std::fs::create_dir_all(&pack).expect("pack");
        let path = pack.join("0001__seed.sql");
        std::fs::write(&path, "CREATE TABLE t (id INTEGER PRIMARY KEY);").expect("seed");

        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");
        apply_private_data_migrations(&pool, dir.path())
            .await
            .expect("initial apply");

        std::fs::write(&path, "CREATE TABLE t (id INTEGER PRIMARY KEY, x INTEGER);")
            .expect("mutate");

        let err = apply_private_data_migrations(&pool, dir.path())
            .await
            .expect_err("drift");
        match err {
            ContentError::MigrationHashDrift { .. } => {}
            other => panic!("unexpected error: {other}"),
        }
    }
}
