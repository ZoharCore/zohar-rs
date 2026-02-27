//! PostgreSQL-specific query implementations.

#[cfg(feature = "db-game")]
use crate::db_types::{DbAppearance, DbEmpire, DbPlayerClass, DbPlayerGender};
#[cfg(feature = "db-game")]
use crate::parse_enum;
#[cfg(feature = "db-auth")]
use crate::traits::AccountRow;
#[cfg(feature = "db-game")]
use crate::traits::{AcquireSessionResult, CreatePlayerOutcome, PlayerRow, ProfileRow};
use crate::{DbContext, DbResult, OptionDbExt};
use sqlx::{PgPool, Row};
#[cfg(feature = "db-game")]
use zohar_domain::Empire as DomainEmpire;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerBaseAppearance as DomainAppearanceVariant;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerClass as DomainPlayerClass;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerGender as DomainPlayerGender;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerId;

#[cfg(feature = "db-auth")]
pub mod auth {
    use super::*;

    pub async fn find_account_by_username(
        pool: &PgPool,
        username: &str,
    ) -> DbResult<Option<AccountRow>> {
        let row = sqlx::query(
            "SELECT username, password_hash FROM auth.accounts WHERE username = $1 LIMIT 1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await
        .db_ctx("query account by username")?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(AccountRow {
            username: row
                .try_get::<String, _>("username")
                .db_ctx("read username")?,
            password_hash: row
                .try_get::<String, _>("password_hash")
                .db_ctx("read password_hash")?,
        }))
    }

    pub async fn update_password(
        pool: &PgPool,
        username: &str,
        password_hash: &str,
    ) -> DbResult<()> {
        sqlx::query("UPDATE auth.accounts SET password_hash = $1 WHERE username = $2")
            .bind(password_hash)
            .bind(username)
            .execute(pool)
            .await
            .db_ctx("update password")?;

        Ok(())
    }
}

#[cfg(feature = "db-game")]
pub mod game {
    use super::*;

    pub async fn find_profile_by_username(
        pool: &PgPool,
        username: &str,
    ) -> DbResult<Option<ProfileRow>> {
        let row = sqlx::query(
            "SELECT username, empire, delete_code, COALESCE((banned_until > NOW()), FALSE) AS is_banned
             FROM game.profiles
             WHERE username = $1
             LIMIT 1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await
        .db_ctx("query profile")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let empire_raw = row
            .try_get::<Option<String>, _>("empire")
            .db_ctx("read empire")?;
        let empire = match empire_raw.as_deref() {
            Some(raw) => Some(parse_enum::<DbEmpire>("empire", raw)?.into()),
            None => None,
        };

        Ok(Some(ProfileRow {
            username: row
                .try_get::<String, _>("username")
                .db_ctx("read username")?,
            empire,
            delete_code: row
                .try_get::<String, _>("delete_code")
                .db_ctx("read delete_code")?,
            is_banned: row
                .try_get::<bool, _>("is_banned")
                .db_ctx("read is_banned")?,
        }))
    }

    pub async fn get_or_create_profile(pool: &PgPool, username: &str) -> DbResult<ProfileRow> {
        sqlx::query("INSERT INTO game.profiles (username) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(username)
            .execute(pool)
            .await
            .db_ctx("ensure profile exists")?;

        find_profile_by_username(pool, username)
            .await?
            .db_invariant("profile should exist after insert")
    }

    pub async fn update_profile_empire(
        pool: &PgPool,
        username: &str,
        empire: DomainEmpire,
    ) -> DbResult<()> {
        sqlx::query("UPDATE game.profiles SET empire = $1 WHERE username = $2")
            .bind(DbEmpire::from(empire).as_ref())
            .bind(username)
            .execute(pool)
            .await
            .db_ctx("update empire")?;

        Ok(())
    }

    pub async fn get_delete_code(pool: &PgPool, username: &str) -> DbResult<Option<String>> {
        let row = sqlx::query("SELECT delete_code FROM game.profiles WHERE username = $1 LIMIT 1")
            .bind(username)
            .fetch_optional(pool)
            .await
            .db_ctx("query delete_code")?;

        row.map(|row| {
            row.try_get::<String, _>("delete_code")
                .db_ctx("read delete_code")
        })
        .transpose()
    }

    fn parse_player_row(row: &sqlx::postgres::PgRow) -> DbResult<PlayerRow> {
        let class_raw = row
            .try_get::<String, _>("class_name")
            .db_ctx("read class_name")?;
        let gender_raw = row.try_get::<String, _>("gender").db_ctx("read gender")?;
        let appearance_raw = row
            .try_get::<String, _>("appearance")
            .db_ctx("read appearance")?;

        Ok(PlayerRow {
            id: PlayerId::from(row.try_get::<i64, _>("id").db_ctx("read id")?),
            username: row
                .try_get::<String, _>("username")
                .db_ctx("read username")?,
            slot: row.try_get::<i32, _>("slot").db_ctx("read slot")?,
            name: row.try_get::<String, _>("name").db_ctx("read name")?,
            level: row.try_get::<i32, _>("level").db_ctx("read level")?,
            class: parse_enum::<DbPlayerClass>("class", &class_raw)?.into(),
            gender: parse_enum::<DbPlayerGender>("gender", &gender_raw)?.into(),
            appearance: parse_enum::<DbAppearance>("appearance", &appearance_raw)?.into(),
            stat_str: row.try_get::<i32, _>("stat_str").db_ctx("read stat_str")?,
            stat_vit: row.try_get::<i32, _>("stat_vit").db_ctx("read stat_vit")?,
            stat_dex: row.try_get::<i32, _>("stat_dex").db_ctx("read stat_dex")?,
            stat_int: row.try_get::<i32, _>("stat_int").db_ctx("read stat_int")?,
            map_key: row
                .try_get::<Option<String>, _>("map_key")
                .db_ctx("read map_key")?,
            local_x: row
                .try_get::<Option<f32>, _>("local_x")
                .db_ctx("read local_x")?,
            local_y: row
                .try_get::<Option<f32>, _>("local_y")
                .db_ctx("read local_y")?,
        })
    }

    const PLAYER_COLS: &str = "id, username, slot, name, level, class_name, gender, appearance, stat_str, stat_vit, stat_dex, stat_int, map_key, local_x, local_y";

    pub async fn list_players_for_user(pool: &PgPool, username: &str) -> DbResult<Vec<PlayerRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {PLAYER_COLS} FROM game.players WHERE username = $1 AND deleted_at IS NULL"
        ))
        .bind(username)
        .fetch_all(pool)
        .await
        .db_ctx("query players")?;

        rows.iter().map(parse_player_row).collect()
    }

    pub async fn find_player_by_slot(
        pool: &PgPool,
        username: &str,
        slot: u8,
    ) -> DbResult<Option<PlayerRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PLAYER_COLS} FROM game.players WHERE username = $1 AND slot = $2 AND deleted_at IS NULL LIMIT 1"
        ))
        .bind(username)
        .bind(i32::from(slot))
        .fetch_optional(pool)
        .await
        .db_ctx("query player by slot")?;

        row.map(|row| parse_player_row(&row)).transpose()
    }

    pub async fn find_player_by_id(pool: &PgPool, id: PlayerId) -> DbResult<Option<PlayerRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PLAYER_COLS} FROM game.players WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        ))
        .bind(i64::from(id))
        .fetch_optional(pool)
        .await
        .db_ctx("query player by id")?;

        row.map(|row| parse_player_row(&row)).transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_player(
        pool: &PgPool,
        username: &str,
        slot: u8,
        name: &str,
        class: DomainPlayerClass,
        gender: DomainPlayerGender,
        appearance: DomainAppearanceVariant,
        stat_str: u8,
        stat_vit: u8,
        stat_dex: u8,
        stat_int: u8,
    ) -> DbResult<CreatePlayerOutcome> {
        let row = sqlx::query(&format!(
            "INSERT INTO game.players (username, slot, name, level, class_name, gender, appearance, stat_str, stat_vit, stat_dex, stat_int) \
             VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT DO NOTHING \
             RETURNING {PLAYER_COLS}"
        ))
        .bind(username)
        .bind(i32::from(slot))
        .bind(name)
        .bind(DbPlayerClass::from(class).as_ref())
        .bind(DbPlayerGender::from(gender).as_ref())
        .bind(DbAppearance::from(appearance).as_ref())
        .bind(i32::from(stat_str))
        .bind(i32::from(stat_vit))
        .bind(i32::from(stat_dex))
        .bind(i32::from(stat_int))
        .fetch_optional(pool)
        .await
        .db_ctx("create player")?;

        let Some(row) = row else {
            return Ok(CreatePlayerOutcome::NameTaken);
        };

        Ok(CreatePlayerOutcome::Created(parse_player_row(&row)?))
    }

    pub async fn delete_player_with_code(
        pool: &PgPool,
        username: &str,
        slot: u8,
        delete_code: &str,
    ) -> DbResult<bool> {
        let row = sqlx::query(
            "WITH profile AS (
                SELECT delete_code FROM game.profiles WHERE username = $1
            ),
            updated AS (
                UPDATE game.players p
                SET deleted_at = NOW()
                FROM profile
                WHERE p.username = $1
                  AND p.slot = $2
                  AND p.deleted_at IS NULL
                  AND profile.delete_code = $3
                RETURNING p.id
            )
            SELECT EXISTS(SELECT 1 FROM updated) AS deleted",
        )
        .bind(username)
        .bind(i32::from(slot))
        .bind(delete_code)
        .fetch_one(pool)
        .await
        .db_ctx("delete player with code")?;

        row.try_get::<bool, _>("deleted")
            .db_ctx("read deleted outcome")
    }

    pub async fn acquire_session(
        pool: &PgPool,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> DbResult<AcquireSessionResult> {
        let row = sqlx::query(
            "INSERT INTO game.sessions (username, server_id, connection_id, last_heartbeat, state)
             VALUES ($1, $2, $3, NOW(), 'ACTIVE')
             ON CONFLICT(username) DO UPDATE SET
                 server_id = CASE
                     WHEN game.sessions.state = 'AUTHED'
                          OR game.sessions.last_heartbeat < NOW() - make_interval(secs => $4::double precision)
                     THEN EXCLUDED.server_id
                     ELSE game.sessions.server_id
                 END,
                 connection_id = CASE
                     WHEN game.sessions.state = 'AUTHED'
                          OR game.sessions.last_heartbeat < NOW() - make_interval(secs => $4::double precision)
                     THEN EXCLUDED.connection_id
                     ELSE game.sessions.connection_id
                 END,
                 last_heartbeat = CASE
                     WHEN game.sessions.state = 'AUTHED'
                          OR game.sessions.last_heartbeat < NOW() - make_interval(secs => $4::double precision)
                     THEN EXCLUDED.last_heartbeat
                     ELSE game.sessions.last_heartbeat
                 END,
                 state = CASE
                     WHEN game.sessions.state = 'AUTHED'
                          OR game.sessions.last_heartbeat < NOW() - make_interval(secs => $4::double precision)
                     THEN 'ACTIVE'
                     ELSE game.sessions.state
                 END
             RETURNING server_id, connection_id",
        )
        .bind(username)
        .bind(server_id)
        .bind(connection_id)
        .bind(stale_threshold_secs)
        .fetch_one(pool)
        .await
        .db_ctx("acquire session")?;

        let result_connection_id = row
            .try_get::<String, _>("connection_id")
            .db_ctx("read connection_id")?;
        if result_connection_id == connection_id {
            Ok(AcquireSessionResult::Acquired)
        } else {
            Ok(AcquireSessionResult::AlreadyOnOtherServer {
                server_id: row
                    .try_get::<String, _>("server_id")
                    .db_ctx("read server_id")?,
            })
        }
    }

    pub async fn resume_session_with_token(
        pool: &PgPool,
        username: &str,
        login_token: u32,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE game.sessions
             SET server_id = $3,
                 connection_id = $4,
                 last_heartbeat = NOW(),
                 state = 'ACTIVE',
                 login_issued_at = NOW(),
                 peer_ip = $6
             WHERE username = $1
               AND login_token = $2
               AND login_issued_at IS NOT NULL
               AND login_issued_at >= NOW() - make_interval(secs => GREATEST($7, 0)::double precision)
               AND (peer_ip IS NULL OR peer_ip = $6)
               AND (
                   state = 'AUTHED'
                   OR last_heartbeat < NOW() - make_interval(secs => GREATEST($5, 0)::double precision)
                   OR (server_id = $3 AND connection_id = $4)
               )",
        )
        .bind(username)
        .bind(i64::from(login_token))
        .bind(server_id)
        .bind(connection_id)
        .bind(stale_threshold_secs)
        .bind(peer_ip)
        .bind(idle_ttl_secs)
        .execute(pool)
        .await
        .db_ctx("resume session with token")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn set_session_login_token(
        pool: &PgPool,
        username: &str,
        login_token: u32,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO game.sessions (
                username,
                server_id,
                connection_id,
                last_heartbeat,
                login_token,
                login_issued_at,
                peer_ip,
                state
             )
             VALUES ($1, NULL, NULL, NOW(), $2, NOW(), NULL, 'AUTHED')
             ON CONFLICT(username) DO UPDATE SET
                login_token = EXCLUDED.login_token,
                login_issued_at = EXCLUDED.login_issued_at,
                peer_ip = NULL,
                state = CASE
                    WHEN game.sessions.state = 'ACTIVE'
                        AND game.sessions.server_id IS NOT NULL
                        AND game.sessions.connection_id IS NOT NULL
                    THEN game.sessions.state
                    ELSE 'AUTHED'
                END,
                server_id = CASE
                    WHEN game.sessions.state = 'ACTIVE'
                        AND game.sessions.server_id IS NOT NULL
                        AND game.sessions.connection_id IS NOT NULL
                    THEN game.sessions.server_id
                    ELSE NULL
                END,
                connection_id = CASE
                    WHEN game.sessions.state = 'ACTIVE'
                        AND game.sessions.server_id IS NOT NULL
                        AND game.sessions.connection_id IS NOT NULL
                    THEN game.sessions.connection_id
                    ELSE NULL
                END,
                last_heartbeat = CASE
                    WHEN game.sessions.state = 'ACTIVE'
                        AND game.sessions.server_id IS NOT NULL
                        AND game.sessions.connection_id IS NOT NULL
                    THEN game.sessions.last_heartbeat
                    ELSE EXCLUDED.last_heartbeat
                END",
        )
        .bind(username)
        .bind(i64::from(login_token))
        .execute(pool)
        .await
        .db_ctx("set session login token")?;

        Ok(())
    }

    pub async fn validate_login_token(
        pool: &PgPool,
        username: &str,
        login_token: u32,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE game.sessions
             SET login_issued_at = NOW(),
                 peer_ip = $4
             WHERE username = $1
               AND login_token = $2
               AND state = 'AUTHED'
               AND login_issued_at IS NOT NULL
               AND login_issued_at >= NOW() - make_interval(secs => GREATEST($3, 0)::double precision)
               AND (peer_ip IS NULL OR peer_ip = $4)",
        )
        .bind(username)
        .bind(i64::from(login_token))
        .bind(idle_ttl_secs)
        .bind(peer_ip)
        .execute(pool)
        .await
        .db_ctx("validate session login token")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_session_stale(
        pool: &PgPool,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE game.sessions
             SET last_heartbeat = NOW() - make_interval(secs => (GREATEST($1, 0) + 1)::double precision),
                 state = 'ACTIVE'
             WHERE username = $2 AND server_id = $3 AND connection_id = $4",
        )
        .bind(stale_threshold_secs)
        .bind(username)
        .bind(server_id)
        .bind(connection_id)
        .execute(pool)
        .await
        .db_ctx("mark session stale")?;

        Ok(())
    }

    pub async fn release_session(pool: &PgPool, username: &str, server_id: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE game.sessions
             SET server_id = NULL, connection_id = NULL, state = 'AUTHED'
             WHERE username = $1 AND server_id = $2",
        )
        .bind(username)
        .bind(server_id)
        .execute(pool)
        .await
        .db_ctx("release session")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn update_session_heartbeat(pool: &PgPool, username: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE game.sessions
             SET last_heartbeat = NOW()
             WHERE username = $1 AND state = 'ACTIVE'",
        )
        .bind(username)
        .execute(pool)
        .await
        .db_ctx("update heartbeat")?;

        Ok(())
    }
}
