use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::empires::EmpireStartConfig;

pub async fn load_empire_start_configs(
    conn: &SqlitePool,
) -> Result<Vec<EmpireStartConfig>, ContentError> {
    let rows =
        sqlx::query("SELECT empire, start_map_id, start_x, start_y FROM empire_start_config")
            .fetch_all(conn)
            .await?;

    rows.into_iter()
        .map(|row| {
            let empire_str: String = row.try_get(0)?;
            let empire = parse_enum(&empire_str, "Empire")?;

            Ok(EmpireStartConfig {
                empire,
                start_map_id: row.try_get(1)?,
                start_x: row.try_get(2)?,
                start_y: row.try_get(3)?,
            })
        })
        .collect()
}
