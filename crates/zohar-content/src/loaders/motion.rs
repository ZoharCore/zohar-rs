use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::motion::ContentHitWindow;
use crate::types::motion::{
    ContentMotion, MotionAction, MotionMode, MotionSetKind, PlayerMotionProfile,
};
use std::collections::HashMap;

pub async fn load_motion(conn: &SqlitePool) -> Result<Vec<ContentMotion>, ContentError> {
    let hit_window_rows =
        sqlx::query("SELECT motion_id, hit_index, start_ms, end_ms FROM motion_hit_window")
            .fetch_all(conn)
            .await?;

    let mut hit_windows: HashMap<i64, Vec<ContentHitWindow>> = HashMap::new();
    for row in hit_window_rows {
        let motion_id: i64 = row.try_get(0)?;
        hit_windows
            .entry(motion_id)
            .or_default()
            .push(ContentHitWindow {
                hit_index: row.try_get(1)?,
                start_ms: row.try_get(2)?,
                end_ms: row.try_get(3)?,
            });
    }

    let rows = sqlx::query(
        "SELECT e.motion_id, e.motion_set_id, s.set_kind,
                sm.mob_id, sp.profile_id,
                e.motion_mode, e.motion_action, e.variant_index, e.weight,
                e.duration_ms, e.accum_x, e.accum_y, e.source
         FROM motion_entry e
         INNER JOIN motion_set s ON s.motion_set_id = e.motion_set_id
         LEFT JOIN motion_set_mob sm ON sm.motion_set_id = e.motion_set_id
         LEFT JOIN motion_set_player_profile sp ON sp.motion_set_id = e.motion_set_id
         ORDER BY e.motion_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_kind: String = row.try_get(2)?;
            let raw_mode: String = row.try_get(5)?;
            let raw_action: String = row.try_get(6)?;
            let set_kind = parse_enum::<MotionSetKind>(&raw_kind, "motion_set_kind")?;
            let motion_mode = parse_enum::<MotionMode>(&raw_mode, "motion_mode")?;
            let motion_action = parse_enum::<MotionAction>(&raw_action, "motion_action")?;

            Ok(ContentMotion {
                motion_id: row.try_get(0)?,
                motion_set_id: row.try_get(1)?,
                set_kind,
                mob_id: row.try_get(3)?,
                profile_id: row.try_get(4)?,
                motion_mode,
                motion_action,
                variant_index: row.try_get(7)?,
                weight: row.try_get(8)?,
                duration_ms: row.try_get(9)?,
                accum_x: row.try_get(10)?,
                accum_y: row.try_get(11)?,
                source: row.try_get(12)?,
                hit_windows: hit_windows
                    .remove(&row.try_get::<i64, _>(0)?)
                    .unwrap_or_default(),
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
