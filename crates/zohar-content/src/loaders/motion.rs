use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::motion::{
    ContentMotion, ContentMotionFlyData, ContentMotionFlyEvent, ContentMotionHitWindow,
    MotionAction, MotionMode, MotionSetKind, PlayerMotionProfile,
};

pub async fn load_motion(conn: &SqlitePool) -> Result<Vec<ContentMotion>, ContentError> {
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
            })
        })
        .collect()
}

pub async fn load_motion_hit_windows(
    conn: &SqlitePool,
) -> Result<Vec<ContentMotionHitWindow>, ContentError> {
    let rows = sqlx::query(
        "SELECT motion_id, hit_index, start_ms, end_ms
         FROM motion_hit_window
         ORDER BY motion_id, hit_index",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ContentMotionHitWindow {
                motion_id: row.try_get(0)?,
                hit_index: row.try_get(1)?,
                start_ms: row.try_get(2)?,
                end_ms: row.try_get(3)?,
            })
        })
        .collect()
}

pub async fn load_motion_fly_events(
    conn: &SqlitePool,
) -> Result<Vec<ContentMotionFlyEvent>, ContentError> {
    let rows = sqlx::query(
        "SELECT motion_id, event_index, release_ms, fly_file
         FROM motion_fly_event
         ORDER BY motion_id, event_index",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ContentMotionFlyEvent {
                motion_id: row.try_get(0)?,
                event_index: row.try_get(1)?,
                release_ms: row.try_get(2)?,
                fly_file: row.try_get(3)?,
            })
        })
        .collect()
}

pub async fn load_motion_fly_data(
    conn: &SqlitePool,
) -> Result<Vec<ContentMotionFlyData>, ContentError> {
    let rows = sqlx::query(
        "SELECT fly_file, init_vel, bomb_range, accel_y, is_homing
         FROM motion_fly_data
         ORDER BY fly_file",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let is_homing: i64 = row.try_get(4)?;
            Ok(ContentMotionFlyData {
                fly_file: row.try_get(0)?,
                init_vel: row.try_get(1)?,
                bomb_range: row.try_get(2)?,
                accel_y: row.try_get(3)?,
                is_homing: is_homing != 0,
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
