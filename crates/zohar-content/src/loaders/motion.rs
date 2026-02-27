use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::motion::{
    ContentMotion, MotionAction, MotionEntityKind, MotionMode, PlayerMotionProfile,
};

pub async fn load_motion(conn: &SqlitePool) -> Result<Vec<ContentMotion>, ContentError> {
    let rows = sqlx::query(
        "SELECT e.motion_id, e.motion_entity_id, ent.entity_kind,
                em.mob_id, ep.profile_id,
                e.motion_mode, e.motion_action, e.duration_ms, e.accum_x, e.accum_y, e.source
         FROM motion_entry e
         INNER JOIN motion_entity ent ON ent.motion_entity_id = e.motion_entity_id
         LEFT JOIN motion_entity_mob em ON em.motion_entity_id = e.motion_entity_id
         LEFT JOIN motion_entity_player ep ON ep.motion_entity_id = e.motion_entity_id
         ORDER BY e.motion_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_kind: String = row.try_get(2)?;
            let raw_mode: String = row.try_get(5)?;
            let raw_action: String = row.try_get(6)?;
            let entity_kind = parse_enum::<MotionEntityKind>(&raw_kind, "motion_entity_kind")?;
            let motion_mode = parse_enum::<MotionMode>(&raw_mode, "motion_mode")?;
            let motion_action = parse_enum::<MotionAction>(&raw_action, "motion_action")?;

            Ok(ContentMotion {
                motion_id: row.try_get(0)?,
                motion_entity_id: row.try_get(1)?,
                entity_kind,
                mob_id: row.try_get(3)?,
                player_profile_id: row.try_get(4)?,
                motion_mode,
                motion_action,
                duration_ms: row.try_get(7)?,
                accum_x: row.try_get(8)?,
                accum_y: row.try_get(9)?,
                source: row.try_get(10)?,
            })
        })
        .collect()
}

pub async fn load_player_motion_profiles(
    conn: &SqlitePool,
) -> Result<Vec<PlayerMotionProfile>, ContentError> {
    let rows = sqlx::query(
        "SELECT profile_id, legacy_race_num, player_class, gender FROM player_motion_profile",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_class: String = row.try_get(2)?;
            let raw_gender: String = row.try_get(3)?;

            let player_class = parse_enum(&raw_class, "PlayerClass")?;
            let gender = parse_enum(&raw_gender, "Gender")?;

            Ok(PlayerMotionProfile {
                profile_id: row.try_get(0)?,
                legacy_race_num: row.try_get(1)?,
                player_class,
                gender,
            })
        })
        .collect()
}
