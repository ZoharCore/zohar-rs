use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

use crate::error::ContentError;
use crate::types::mob_groups::{
    MobGroupEntry, MobGroupGroupEntry, MobGroupGroupRecord, MobGroupRecord,
};

pub async fn load_mob_groups(conn: &SqlitePool) -> Result<Vec<MobGroupRecord>, ContentError> {
    let rows = sqlx::query(
        "SELECT g.group_id, g.code, e.seq, e.mob_id
         FROM mob_group g
         LEFT JOIN mob_group_entry e ON e.group_id = g.group_id
         ORDER BY g.group_id, e.seq",
    )
    .fetch_all(conn)
    .await?;

    let mut by_group: BTreeMap<i64, MobGroupRecord> = BTreeMap::new();

    for row in rows {
        let group_id: i64 = row.try_get(0)?;
        let code: Option<String> = row.try_get(1)?;
        let mob_id: Option<i64> = row.try_get(3)?;
        let group = by_group.entry(group_id).or_insert_with(|| MobGroupRecord {
            group_id,
            code,
            entries: Vec::new(),
        });
        if let Some(mob_id) = mob_id {
            group.entries.push(MobGroupEntry { mob_id });
        }
    }

    Ok(by_group.into_values().collect())
}

pub async fn load_mob_group_groups(
    conn: &SqlitePool,
) -> Result<Vec<MobGroupGroupRecord>, ContentError> {
    let rows = sqlx::query(
        "SELECT gg.group_group_id, gg.code, e.seq, e.group_id, e.weight
         FROM mob_group_group gg
         LEFT JOIN mob_group_group_entry e ON e.group_group_id = gg.group_group_id
         ORDER BY gg.group_group_id, e.seq",
    )
    .fetch_all(conn)
    .await?;

    let mut by_group_group: BTreeMap<i64, MobGroupGroupRecord> = BTreeMap::new();

    for row in rows {
        let group_group_id: i64 = row.try_get(0)?;
        let code: Option<String> = row.try_get(1)?;
        let group_id: Option<i64> = row.try_get(3)?;
        let weight: Option<i64> = row.try_get(4)?;
        let group_group =
            by_group_group
                .entry(group_group_id)
                .or_insert_with(|| MobGroupGroupRecord {
                    group_group_id,
                    code,
                    entries: Vec::new(),
                });
        if let Some(group_id) = group_id {
            group_group.entries.push(MobGroupGroupEntry {
                group_id,
                weight: weight.unwrap_or(1),
            });
        }
    }

    Ok(by_group_group.into_values().collect())
}
