use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum, parse_flags};
use crate::types::mobs::{ContentMob, MobAiFlags, MobBattleType, MobRank, MobType};

pub async fn load_mobs(conn: &SqlitePool) -> Result<Vec<ContentMob>, ContentError> {
    let rows = sqlx::query(
        "SELECT mob_id, code, name, mob_type, rank, battle_type, level, ai_flags, move_speed,
                attack_speed, aggressive_sight, attack_range, strength, dexterity, vitality,
                intelligence, damage_min, damage_max, max_hp, defense, damage_multiplier
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

            let raw_battle_type: String = row.try_get(5)?;
            let battle_type = parse_enum::<MobBattleType>(&raw_battle_type, "battle_type")?;

            let raw_ai_flags: Option<String> = row.try_get(7)?;
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
                battle_type,
                level: row.try_get(6)?,
                ai_flags,
                move_speed: row.try_get(8)?,
                attack_speed: row.try_get(9)?,
                aggressive_sight: row.try_get(10)?,
                attack_range: row.try_get(11)?,
                strength: row.try_get(12)?,
                dexterity: row.try_get(13)?,
                vitality: row.try_get(14)?,
                intelligence: row.try_get(15)?,
                damage_min: row.try_get(16)?,
                damage_max: row.try_get(17)?,
                max_hp: row.try_get(18)?,
                defense: row.try_get(19)?,
                damage_multiplier: row.try_get(20)?,
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
        sqlx::query("INSERT INTO enum_battle_type (value) VALUES ('MELEE'), ('RANGE')")
            .execute(&pool)
            .await
            .expect("battle types");

        sqlx::query(
            "INSERT INTO mob_proto (
                mob_id, code, name, mob_type, rank, battle_type, level, ai_flags, move_speed,
                attack_speed, aggressive_sight, attack_range, strength, dexterity, vitality,
                intelligence, damage_min, damage_max, max_hp, defense, damage_multiplier
             )
             VALUES (101, 'MOB_101', 'wild dog', 'MONSTER', 'PAWN', 'MELEE', 1, 'NOMOVE|AGGR', 100, 100, 800, 175, 3, 6, 5, 2, 20, 24, 126, 4, 1.0),
                    (102, 'MOB_102', 'stable boy', 'NPC', 'KING', 'RANGE', 70, NULL, 100, 100, 0, 300, 4, 9, 7, 2, 23, 28, 162, 6, 1.0)",
        )
        .execute(&pool)
        .await
        .expect("seed mobs");

        let mobs = load_mobs(&pool).await.expect("mobs");
        assert_eq!(mobs.len(), 2);
        assert_eq!(mobs[0].ai_flags, MobAiFlags::NOMOVE | MobAiFlags::AGGR);
        assert_eq!(mobs[0].aggressive_sight, 800);
        assert_eq!(mobs[0].attack_range, 175);
        assert_eq!(mobs[0].strength, 3);
        assert_eq!(mobs[0].damage_min, 20);
        assert_eq!(mobs[0].damage_max, 24);
        assert_eq!(mobs[0].max_hp, 126);
        assert_eq!(mobs[0].defense, 4);
        assert_eq!(mobs[0].damage_multiplier, 1.0);
        assert_eq!(mobs[1].ai_flags, MobAiFlags::empty());
    }
}
