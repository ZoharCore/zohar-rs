use sqlx::SqlitePool;

use crate::types::ContentCatalog;

#[derive(Debug, Clone)]
pub struct AppliedMigration {
    pub id: String,
    pub hash: String,
    pub rejected_count: usize,
}

#[derive(Debug, Clone)]
pub struct RejectedStatement {
    pub migration_id: String,
    pub statement_index: usize,
    pub error: String,
}

#[derive(Debug, Default, Clone)]
pub struct MigrationSummary {
    pub schema_applied: Vec<AppliedMigration>,
    pub data_applied: Vec<AppliedMigration>,
    pub rejected_statements: Vec<RejectedStatement>,
}

#[derive(Debug)]
pub struct ContentRuntime {
    conn: SqlitePool,
    catalog: ContentCatalog,
    migration_summary: MigrationSummary,
}

impl ContentRuntime {
    pub(crate) fn new(
        conn: SqlitePool,
        catalog: ContentCatalog,
        migration_summary: MigrationSummary,
    ) -> Self {
        Self {
            conn,
            catalog,
            migration_summary,
        }
    }

    pub fn catalog(&self) -> &ContentCatalog {
        &self.catalog
    }

    pub fn migration_summary(&self) -> &MigrationSummary {
        &self.migration_summary
    }

    pub fn connection(&self) -> &SqlitePool {
        &self.conn
    }
}
