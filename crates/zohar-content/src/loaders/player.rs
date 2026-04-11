use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::player::PlayerClassBaseStats;

pub async fn load_player_class_base_stats(
    conn: &SqlitePool,
) -> Result<Vec<PlayerClassBaseStats>, ContentError> {
    let rows = sqlx::query(
        "SELECT class_key,
                base_strength, base_vitality, base_dexterity, base_intelligence,
                base_hp, base_sp,
                hp_per_vitality, sp_per_intelligence,
                hp_per_level_min, hp_per_level_max,
                sp_per_level_min, sp_per_level_max,
                base_stamina, stamina_per_vitality,
                stamina_per_level_min, stamina_per_level_max
         FROM player_class_base_stats
         ORDER BY class_key",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_class: String = row.try_get(0)?;
            Ok(PlayerClassBaseStats {
                player_class: parse_enum(&raw_class, "PlayerClass")?,
                base_strength: row.try_get(1)?,
                base_vitality: row.try_get(2)?,
                base_dexterity: row.try_get(3)?,
                base_intelligence: row.try_get(4)?,
                base_hp: row.try_get(5)?,
                base_sp: row.try_get(6)?,
                hp_per_vitality: row.try_get(7)?,
                sp_per_intelligence: row.try_get(8)?,
                hp_per_level_min: row.try_get(9)?,
                hp_per_level_max: row.try_get(10)?,
                sp_per_level_min: row.try_get(11)?,
                sp_per_level_max: row.try_get(12)?,
                base_stamina: row.try_get(13)?,
                stamina_per_vitality: row.try_get(14)?,
                stamina_per_level_min: row.try_get(15)?,
                stamina_per_level_max: row.try_get(16)?,
            })
        })
        .collect()
}
