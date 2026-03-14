use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use tracing::warn;

use crate::error::ContentError;
use crate::types::maps::{ContentMap, MapTownSpawn, TerrainFlags, TerrainFlagsGrid};

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

pub async fn load_map_flag_grids(
    conn: &SqlitePool,
    maps: &[ContentMap],
) -> Result<Vec<TerrainFlagsGrid>, ContentError> {
    let rows = sqlx::query(
        "SELECT map_id, cell_size_m, codec, raw_len, data FROM map_terrain_flags ORDER BY map_id",
    )
    .fetch_all(conn)
    .await?;

    let map_dims: HashMap<i64, (f32, f32)> = maps
        .iter()
        .map(|map| (map.map_id, (map.map_width, map.map_height)))
        .collect();

    let mut out = Vec::new();
    for row in rows {
        let map_id: i64 = row.try_get(0)?;
        let Some((map_width, map_height)) = map_dims.get(&map_id).copied() else {
            warn!(map_id, "Skipping map_terrain_flags row for unknown map");
            continue;
        };

        let cell_size_m: f32 = row.try_get(1)?;
        let codec: String = row.try_get(2)?;
        let raw_len: i64 = row.try_get(3)?;
        let data: Vec<u8> = row.try_get(4)?;

        let Some(grid_width) = derive_axis_len(map_width, cell_size_m) else {
            warn!(
                map_id,
                map_width,
                cell_size_m,
                "Skipping map_terrain_flags row with non-integral derived grid width"
            );
            continue;
        };
        let Some(grid_height) = derive_axis_len(map_height, cell_size_m) else {
            warn!(
                map_id,
                map_height,
                cell_size_m,
                "Skipping map_terrain_flags row with non-integral derived grid height"
            );
            continue;
        };

        let expected_raw_len = match grid_width.checked_mul(grid_height) {
            Some(value) => value,
            None => {
                warn!(
                    map_id,
                    grid_width, grid_height, "Skipping oversized map_terrain_flags row"
                );
                continue;
            }
        };

        if raw_len < 0 || raw_len as usize != expected_raw_len {
            warn!(
                map_id,
                raw_len,
                expected_raw_len,
                "Skipping map_terrain_flags row with mismatched raw length"
            );
            continue;
        }

        let decoded = match decode_payload(&codec, &data) {
            Some(bytes) => bytes,
            None => {
                warn!(
                    map_id,
                    codec, "Skipping map_terrain_flags row with undecodable payload"
                );
                continue;
            }
        };

        if decoded.len() != expected_raw_len {
            warn!(
                map_id,
                decoded_len = decoded.len(),
                expected_raw_len,
                "Skipping map_terrain_flags row with decoded length mismatch"
            );
            continue;
        }

        let decoded_flags = decoded
            .into_iter()
            .map(TerrainFlags::from_bits_retain)
            .collect();

        out.push(TerrainFlagsGrid {
            map_id,
            cell_size_m,
            grid_width,
            grid_height,
            data: decoded_flags,
        });
    }

    Ok(out)
}

fn derive_axis_len(map_extent_m: f32, cell_size_m: f32) -> Option<usize> {
    if !map_extent_m.is_finite()
        || !cell_size_m.is_finite()
        || map_extent_m <= 0.0
        || cell_size_m <= 0.0
    {
        return None;
    }

    let quotient = f64::from(map_extent_m) / f64::from(cell_size_m);
    let rounded = quotient.round();
    if (quotient - rounded).abs() > 1e-6 || rounded <= 0.0 || rounded > usize::MAX as f64 {
        return None;
    }

    Some(rounded as usize)
}

fn decode_payload(codec: &str, payload: &[u8]) -> Option<Vec<u8>> {
    match codec {
        "NONE" => Some(payload.to_vec()),
        "ZSTD" => zstd::stream::decode_all(payload).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_fresh_connection;
    use crate::migrations::schema::apply_schema_migrations;

    async fn setup_pool() -> (tempfile::TempDir, SqlitePool) {
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
        (dir, pool)
    }

    async fn setup_pool_without_triggers() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("conn");
        sqlx::query(
            "CREATE TABLE map_def (
                map_id INTEGER PRIMARY KEY,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                map_width REAL NOT NULL,
                map_height REAL NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("map_def");
        sqlx::query(
            "CREATE TABLE map_terrain_flags (
                map_id INTEGER PRIMARY KEY,
                cell_size_m REAL NOT NULL,
                codec TEXT NOT NULL,
                raw_len INTEGER NOT NULL,
                data BLOB NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("map_terrain_flags");
        (dir, pool)
    }

    fn maps_fixture() -> Vec<ContentMap> {
        vec![ContentMap {
            map_id: 1,
            code: "map_a".to_string(),
            name: "Map A".to_string(),
            map_width: 1024.0,
            map_height: 1280.0,
            empire: None,
            base_x: Some(0.0),
            base_y: Some(0.0),
        }]
    }

    #[tokio::test]
    async fn loads_none_payload_terrain_flags() {
        let (_dir, pool) = setup_pool().await;
        let raw = vec![1u8; 1024 * 1280 * 4];
        sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.5, 'NONE', ?1, ?2)",
        )
        .bind(raw.len() as i64)
        .bind(raw.clone())
        .execute(&pool)
        .await
        .expect("insert terrain_flags");

        let grids = load_map_flag_grids(&pool, &maps_fixture())
            .await
            .expect("grids");
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].grid_width, 2048);
        assert_eq!(grids[0].grid_height, 2560);
        assert_eq!(grids[0].data, vec![TerrainFlags::BLOCK; raw.len()]);
    }

    #[tokio::test]
    async fn loads_zstd_payload_terrain_flags() {
        let (_dir, pool) = setup_pool().await;
        let raw = vec![0x80u8; 2048 * 2560];
        let compressed = zstd::stream::encode_all(raw.as_slice(), 3).expect("zstd");
        sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.5, 'ZSTD', ?1, ?2)",
        )
        .bind(raw.len() as i64)
        .bind(compressed)
        .execute(&pool)
        .await
        .expect("insert terrain_flags");

        let grids = load_map_flag_grids(&pool, &maps_fixture())
            .await
            .expect("grids");
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].data, vec![TerrainFlags::OBJECT; raw.len()]);
    }

    #[tokio::test]
    async fn skips_non_integral_grid_dimensions() {
        let (_dir, pool) = setup_pool_without_triggers().await;
        sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.3, 'NONE', 4, X'01020304')",
        )
        .execute(&pool)
        .await
        .expect("insert terrain_flags");

        let grids = load_map_flag_grids(&pool, &maps_fixture())
            .await
            .expect("grids");
        assert!(grids.is_empty());
    }

    #[tokio::test]
    async fn skips_raw_length_mismatch() {
        let (_dir, pool) = setup_pool_without_triggers().await;
        sqlx::query(
            "INSERT INTO map_terrain_flags (map_id, cell_size_m, codec, raw_len, data)
             VALUES (1, 0.5, 'NONE', 3, X'01020304')",
        )
        .execute(&pool)
        .await
        .expect("insert terrain_flags");

        let grids = load_map_flag_grids(&pool, &maps_fixture())
            .await
            .expect("grids");
        assert!(grids.is_empty());
    }
}
