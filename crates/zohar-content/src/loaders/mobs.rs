use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::mobs::{ContentMob, MobRank, MobType};

pub async fn load_mobs(conn: &SqlitePool) -> Result<Vec<ContentMob>, ContentError> {
    let rows = sqlx::query(
        "SELECT mob_id, code, name, mob_type, rank, level, move_speed, attack_speed
         FROM mob_proto
         ORDER BY mob_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_type: String = row.try_get(3)?;
            let mob_type = parse_enum::<MobType>(&raw_type, "mob_type")?;

            let raw_rank: String = row.try_get(4)?;
            let rank = parse_enum::<MobRank>(&raw_rank, "rank")?;

            Ok(ContentMob {
                mob_id: row.try_get(0)?,
                code: row.try_get(1)?,
                name: row.try_get(2)?,
                mob_type,
                rank,
                level: row.try_get(5)?,
                move_speed: row.try_get(6)?,
                attack_speed: row.try_get(7)?,
            })
        })
        .collect()
}
