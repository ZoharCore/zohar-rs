use sqlx::{Row, SqlitePool};

use crate::error::{ContentError, parse_enum};
use crate::types::chat::{MobChatLine, MobChatStrategy};
use crate::types::mobs::MobType;

pub async fn load_mob_chat_strategies(
    conn: &SqlitePool,
) -> Result<Vec<MobChatStrategy>, ContentError> {
    let rows = sqlx::query(
        "SELECT chat_context, mob_type, mob_id, interval_min_sec, interval_max_sec
         FROM mob_chat_strategy
         ORDER BY chat_context, mob_type, mob_id",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let raw_mob_type: Option<String> = row.try_get(1)?;
            let mob_type = raw_mob_type
                .as_deref()
                .map(|raw| parse_enum::<MobType>(raw, "mob_chat_strategy.mob_type"))
                .transpose()?;

            Ok(MobChatStrategy {
                chat_context: row.try_get(0)?,
                mob_type,
                mob_id: row.try_get(2)?,
                interval_min_sec: row.try_get(3)?,
                interval_max_sec: row.try_get(4)?,
            })
        })
        .collect()
}

pub async fn load_mob_chat_lines(conn: &SqlitePool) -> Result<Vec<MobChatLine>, ContentError> {
    let rows = sqlx::query(
        "SELECT mob_id, chat_context, source_key, text
         FROM mob_chat_line
         ORDER BY mob_id, chat_context, source_key",
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(MobChatLine {
                mob_id: row.try_get(0)?,
                chat_context: row.try_get(1)?,
                source_key: row.try_get(2)?,
                text: row.try_get(3)?,
            })
        })
        .collect()
}
