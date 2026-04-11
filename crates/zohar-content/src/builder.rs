use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

use crate::db::{open_existing_read_only, open_fresh_connection};
use crate::error::ContentError;
use crate::loaders::{chat, empires, maps, mob_groups, mobs, motion, player, spawns};
use crate::migrations::{private_data, schema};
use crate::runtime::{ContentRuntime, MigrationSummary};
use crate::types::ContentCatalog;

const DEFAULT_DB_PATH: &str = "content.db";
const PRIVATE_CONTENT_ROOT: &str = "data/content";

#[derive(Debug, Default, Clone)]
pub struct ContentRuntimeBuilder {
    db_path: Option<PathBuf>,
}

impl ContentRuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn db_path(mut self, path: PathBuf) -> Self {
        self.db_path = Some(path);
        self
    }

    pub async fn run(self) -> Result<ContentRuntime, ContentError> {
        self.run_with_private_root(Path::new(PRIVATE_CONTENT_ROOT))
            .await
    }

    pub async fn run_read_only(self) -> Result<ContentRuntime, ContentError> {
        let db_path = self
            .db_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DB_PATH));
        let conn = open_existing_read_only(&db_path).await?;
        let catalog = load_catalog(&conn).await?;
        Ok(ContentRuntime::new(
            conn,
            catalog,
            MigrationSummary::default(),
        ))
    }

    pub(crate) async fn run_with_private_root(
        self,
        private_root: &Path,
    ) -> Result<ContentRuntime, ContentError> {
        let db_path = self
            .db_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DB_PATH));
        let conn = open_fresh_connection(&db_path).await?;

        let schema_applied = schema::apply_schema_migrations(&conn).await?;
        let (data_applied, rejected_statements) =
            private_data::apply_private_data_migrations(&conn, private_root).await?;

        let catalog = load_catalog(&conn).await?;

        let summary = MigrationSummary {
            schema_applied,
            data_applied,
            rejected_statements,
        };

        Ok(ContentRuntime::new(conn, catalog, summary))
    }
}

async fn load_catalog(conn: &SqlitePool) -> Result<ContentCatalog, ContentError> {
    let maps = maps::load_maps(conn).await?;
    let map_terrain_flags = maps::load_map_flag_grids(conn, &maps).await?;

    Ok(ContentCatalog {
        player_class_base_stats: player::load_player_class_base_stats(conn).await?,
        maps,
        map_terrain_flags,
        town_spawns: maps::load_town_spawns(conn).await?,
        mobs: mobs::load_mobs(conn).await?,
        mob_groups: mob_groups::load_mob_groups(conn).await?,
        mob_group_groups: mob_groups::load_mob_group_groups(conn).await?,
        player_motion_profiles: motion::load_player_motion_profiles(conn).await?,
        empire_start_configs: empires::load_empire_start_configs(conn).await?,
        spawn_rules: spawns::load_spawn_rules(conn).await?,
        motion: motion::load_motion(conn).await?,
        mob_chat_strategies: chat::load_mob_chat_strategies(conn).await?,
        mob_chat_lines: chat::load_mob_chat_lines(conn).await?,
    })
}
