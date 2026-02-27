use sqlx::{Row, SqlitePool};

use crate::error::ContentError;
use crate::types::maps::{ContentMap, MapTownSpawn};

pub async fn load_maps(conn: &SqlitePool) -> Result<Vec<ContentMap>, ContentError> {
    let rows = sqlx::query(
        "SELECT d.map_id, d.code, d.name, d.map_width, d.map_height,
                d.empire, p.base_x, p.base_y
         FROM map_def d
         LEFT JOIN map_placement p ON p.map_id = d.map_id
         ORDER BY d.map_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let empire_str: Option<String> = row.try_get(5)?;
            let empire = match empire_str {
                Some(s) => Some(crate::error::parse_enum(&s, "Empire")?),
                None => None,
            };

            Ok(ContentMap {
                map_id: row.try_get(0)?,
                code: row.try_get(1)?,
                name: row.try_get(2)?,
                map_width: row.try_get(3)?,
                map_height: row.try_get(4)?,
                empire,
                base_x: row.try_get(6)?,
                base_y: row.try_get(7)?,
            })
        })
        .collect()
}

pub async fn load_town_spawns(conn: &SqlitePool) -> Result<Vec<MapTownSpawn>, ContentError> {
    let rows = sqlx::query("SELECT map_id, empire, x, y FROM map_town_spawn")
        .fetch_all(conn)
        .await?;

    rows.into_iter()
        .map(|row| {
            let empire_str: String = row.try_get(1)?;
            let empire = crate::error::parse_enum(&empire_str, "Empire")?;

            Ok(MapTownSpawn {
                map_id: row.try_get(0)?,
                empire,
                x: row.try_get(2)?,
                y: row.try_get(3)?,
            })
        })
        .collect()
}
