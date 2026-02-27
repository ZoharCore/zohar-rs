use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::spawns::{SpawnRuleRecord, SpawnSource, SpawnTarget, SpawnType};

pub async fn load_spawn_rules(conn: &SqlitePool) -> Result<Vec<SpawnRuleRecord>, ContentError> {
    let rows = sqlx::query(
        "SELECT r.map_id, d.code, r.target_mob_id, r.target_group_id, r.target_group_group_id,
                r.spawn_type, r.spawn_source,
                r.center_x, r.center_y, r.extent_x, r.extent_y, r.direction,
                r.regen_time_sec, r.regen_percent, r.max_count
         FROM map_spawn_rule r
         INNER JOIN map_def d ON d.map_id = r.map_id
         ORDER BY r.map_id, r.spawn_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_spawn_type: String = row.try_get(5)?;
            let raw_spawn_source: String = row.try_get(6)?;
            let spawn_type = parse_enum::<SpawnType>(&raw_spawn_type, "spawn_type")?;
            let spawn_source = parse_enum::<SpawnSource>(&raw_spawn_source, "spawn_source")?;

            let target_mob_id: Option<i64> = row.try_get(2)?;
            let target_group_id: Option<i64> = row.try_get(3)?;
            let target_group_group_id: Option<i64> = row.try_get(4)?;

            let target = match (target_mob_id, target_group_id, target_group_group_id) {
                (Some(mob_id), None, None) => SpawnTarget::Mob(mob_id),
                (None, Some(group_id), None) => SpawnTarget::Group(group_id),
                (None, None, Some(group_group_id)) => SpawnTarget::GroupGroup(group_group_id),
                _ => {
                    return Err(ContentError::InvalidEnum {
                        kind: "spawn_target",
                        value: format!(
                            "mob={target_mob_id:?},group={target_group_id:?},group_group={target_group_group_id:?}"
                        ),
                    });
                }
            };

            Ok(SpawnRuleRecord {
                map_id: row.try_get(0)?,
                map_code: row.try_get(1)?,
                target,
                spawn_type,
                spawn_source,
                center_x: row.try_get(7)?,
                center_y: row.try_get(8)?,
                extent_x: row.try_get(9)?,
                extent_y: row.try_get(10)?,
                direction: row.try_get(11)?,
                regen_time_sec: row.try_get(12)?,
                regen_percent: row.try_get(13)?,
                max_count: row.try_get(14)?,
            })
        })
        .collect()
}
