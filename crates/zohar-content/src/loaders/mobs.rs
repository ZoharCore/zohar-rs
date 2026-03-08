use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum, parse_flags};
use crate::types::mobs::{ContentMob, MobAiFlags, MobRank, MobType};

pub async fn load_mobs(conn: &SqlitePool) -> Result<Vec<ContentMob>, ContentError> {
    let rows = sqlx::query(
        "SELECT mob_id, code, name, mob_type, rank, level, ai_flags, move_speed, attack_speed
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

            let raw_ai_flags: Option<String> = row.try_get(6)?;
            let ai_flags = match raw_ai_flags.as_deref().map(str::trim) {
                None | Some("") => MobAiFlags::empty(),
                Some(value) => parse_flags::<MobAiFlags>(value, "ai_flags")?,
            };

            Ok(ContentMob {
                mob_id: row.try_get(0)?,
                code: row.try_get(1)?,
                name: row.try_get(2)?,
                mob_type,
                rank,
                level: row.try_get(5)?,
                ai_flags,
                move_speed: row.try_get(7)?,
                attack_speed: row.try_get(8)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::load_mobs;
    use crate::db::open_fresh_connection;
    use crate::migrations::schema::apply_schema_migrations;
    use crate::types::mobs::MobAiFlags;
    use tempfile::tempdir;

    #[tokio::test]
    async fn load_mobs_parses_ai_flags_and_empty_values() {
        let dir = tempdir().expect("tempdir");
        let pool = open_fresh_connection(&dir.path().join("content.db"))
            .await
            .expect("pool");
        apply_schema_migrations(&pool).await.expect("schema");

        sqlx::query("INSERT INTO enum_mob_type (value) VALUES ('NPC'), ('MONSTER')")
            .execute(&pool)
            .await
            .expect("mob types");
        sqlx::query("INSERT INTO enum_mob_rank (value) VALUES ('PAWN'), ('KING')")
            .execute(&pool)
            .await
            .expect("mob ranks");

        sqlx::query(
            "INSERT INTO mob_proto (mob_id, code, name, mob_type, rank, level, ai_flags, move_speed, attack_speed)
             VALUES (101, 'MOB_101', 'wild dog', 'MONSTER', 'PAWN', 1, 'NOMOVE|AGGR', 100, 100),
                    (102, 'MOB_102', 'stable boy', 'NPC', 'KING', 70, NULL, 100, 100)",
        )
        .execute(&pool)
        .await
        .expect("seed mobs");

        let mobs = load_mobs(&pool).await.expect("mobs");
        assert_eq!(mobs.len(), 2);
        assert_eq!(mobs[0].ai_flags, MobAiFlags::NOMOVE | MobAiFlags::AGGR);
        assert_eq!(mobs[1].ai_flags, MobAiFlags::empty());
    }
}
