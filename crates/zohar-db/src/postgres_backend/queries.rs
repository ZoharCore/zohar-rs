//! PostgreSQL-specific query implementations.

#[cfg(feature = "db-game")]
use crate::db_types::{DbAppearance, DbEmpire, DbPlayerClass, DbPlayerGender};
#[cfg(feature = "db-game")]
use crate::parse_enum;
#[cfg(feature = "db-auth")]
use crate::traits::AccountRow;
#[cfg(feature = "db-game")]
use crate::traits::{
    AcquireSessionResult, CreatePlayerOutcome, PlayerCoreStatAllocationRow, PlayerRuntimeStateRow,
    PlayerStatsBootstrapRow, PlayerSummaryRow, PlayerWriteOutcome, ProfileRow,
};
use crate::{DbContext, DbResult, OptionDbExt};
use sqlx::{PgPool, Row};
#[cfg(feature = "db-game")]
use zohar_domain::Empire as DomainEmpire;
#[cfg(feature = "db-game")]
use zohar_domain::PlayerExitKind;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerBaseAppearance as DomainAppearanceVariant;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerClass as DomainPlayerClass;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerGender as DomainPlayerGender;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerSnapshot;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::{PlayerId, PlayerRuntimeEpoch};

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

    fn parse_player_class(row: &sqlx::postgres::PgRow) -> DbResult<DomainPlayerClass> {
        let class_raw = row
            .try_get::<String, _>("class_name")
            .db_ctx("read class_name")?;
        parse_enum::<DbPlayerClass>("class", &class_raw).map(Into::into)
    }

    fn parse_player_gender(row: &sqlx::postgres::PgRow) -> DbResult<DomainPlayerGender> {
        let gender_raw = row.try_get::<String, _>("gender").db_ctx("read gender")?;
        parse_enum::<DbPlayerGender>("gender", &gender_raw).map(Into::into)
    }

    fn parse_player_appearance(row: &sqlx::postgres::PgRow) -> DbResult<DomainAppearanceVariant> {
        let appearance_raw = row
            .try_get::<String, _>("appearance")
            .db_ctx("read appearance")?;
        parse_enum::<DbAppearance>("appearance", &appearance_raw).map(Into::into)
    }

    fn parse_player_summary_row(row: &sqlx::postgres::PgRow) -> DbResult<PlayerSummaryRow> {
        Ok(PlayerSummaryRow {
            id: PlayerId::from(row.try_get::<i64, _>("id").db_ctx("read id")?),
            username: row
                .try_get::<String, _>("username")
                .db_ctx("read username")?,
            slot: row.try_get::<i32, _>("slot").db_ctx("read slot")?,
            name: row.try_get::<String, _>("name").db_ctx("read name")?,
            class: parse_player_class(row)?,
            gender: parse_player_gender(row)?,
            appearance: parse_player_appearance(row)?,
            level: row.try_get::<i32, _>("level").db_ctx("read level")?,
            playtime_secs: row
                .try_get::<i64, _>("playtime_secs")
                .db_ctx("read playtime_secs")?,
            core_stat_allocations: parse_player_core_stat_allocation_row(row)?,
        })
    }

    fn parse_player_stats_bootstrap_row(
        row: &sqlx::postgres::PgRow,
    ) -> DbResult<PlayerStatsBootstrapRow> {
        Ok(PlayerStatsBootstrapRow {
            id: PlayerId::from(row.try_get::<i64, _>("id").db_ctx("read id")?),
            username: row
                .try_get::<String, _>("username")
                .db_ctx("read username")?,
            slot: row.try_get::<i32, _>("slot").db_ctx("read slot")?,
            name: row.try_get::<String, _>("name").db_ctx("read name")?,
            class: parse_player_class(row)?,
            gender: parse_player_gender(row)?,
            appearance: parse_player_appearance(row)?,
            level: row.try_get::<i32, _>("level").db_ctx("read level")?,
            exp_in_level: row
                .try_get::<i64, _>("exp_in_level")
                .db_ctx("read exp_in_level")?,
            core_stat_allocations: parse_player_core_stat_allocation_row(row)?,
            stat_reset_count: row
                .try_get::<i32, _>("stat_reset_count")
                .db_ctx("read stat_reset_count")?,
            playtime_secs: row
                .try_get::<i64, _>("playtime_secs")
                .db_ctx("read playtime_secs")?,
            current_hp: row
                .try_get::<Option<i32>, _>("current_hp")
                .db_ctx("read current_hp")?,
            current_sp: row
                .try_get::<Option<i32>, _>("current_sp")
                .db_ctx("read current_sp")?,
            current_stamina: row
                .try_get::<Option<i32>, _>("current_stamina")
                .db_ctx("read current_stamina")?,
        })
    }

    fn parse_player_core_stat_allocation_row(
        row: &sqlx::postgres::PgRow,
    ) -> DbResult<PlayerCoreStatAllocationRow> {
        Ok(PlayerCoreStatAllocationRow {
            allocated_str: row
                .try_get::<i32, _>("allocated_str")
                .db_ctx("read allocated_str")?,
            allocated_vit: row
                .try_get::<i32, _>("allocated_vit")
                .db_ctx("read allocated_vit")?,
            allocated_dex: row
                .try_get::<i32, _>("allocated_dex")
                .db_ctx("read allocated_dex")?,
            allocated_int: row
                .try_get::<i32, _>("allocated_int")
                .db_ctx("read allocated_int")?,
        })
    }

    fn parse_player_runtime_state_row(
        row: &sqlx::postgres::PgRow,
    ) -> DbResult<PlayerRuntimeStateRow> {
        Ok(PlayerRuntimeStateRow {
            player_id: PlayerId::from(
                row.try_get::<i64, _>("player_id")
                    .db_ctx("read player_id")?,
            ),
            map_key: row
                .try_get::<Option<String>, _>("map_key")
                .db_ctx("read map_key")?,
            local_x: row
                .try_get::<Option<f32>, _>("local_x")
                .db_ctx("read local_x")?,
            local_y: row
                .try_get::<Option<f32>, _>("local_y")
                .db_ctx("read local_y")?,
            current_hp: row
                .try_get::<Option<i32>, _>("current_hp")
                .db_ctx("read current_hp")?,
            current_sp: row
                .try_get::<Option<i32>, _>("current_sp")
                .db_ctx("read current_sp")?,
            current_stamina: row
                .try_get::<Option<i32>, _>("current_stamina")
                .db_ctx("read current_stamina")?,
            runtime_epoch: PlayerRuntimeEpoch::from(
                row.try_get::<i64, _>("runtime_epoch")
                    .db_ctx("read runtime_epoch")?,
            ),
        })
    }

    const PLAYER_SUMMARY_COLS: &str = "p.id, p.username, p.slot, p.name, p.class_name, p.gender, p.appearance, prog.level, prog.playtime_secs, prog.allocated_str, prog.allocated_vit, prog.allocated_dex, prog.allocated_int";
    const PLAYER_RUNTIME_COLS: &str = "runtime.player_id, runtime.map_key, runtime.local_x, runtime.local_y, runtime.current_hp, runtime.current_sp, runtime.current_stamina, runtime.runtime_epoch";

    pub async fn list_player_summaries_for_user(
        pool: &PgPool,
        username: &str,
    ) -> DbResult<Vec<PlayerSummaryRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {PLAYER_SUMMARY_COLS}
             FROM game.players p
             JOIN game.player_progression prog ON prog.player_id = p.id
             WHERE p.username = $1
               AND p.deleted_at IS NULL
             ORDER BY p.slot"
        ))
        .bind(username)
        .fetch_all(pool)
        .await
        .db_ctx("query player summaries")?;

        rows.iter().map(parse_player_summary_row).collect()
    }

    pub async fn find_player_summary_by_slot(
        pool: &PgPool,
        username: &str,
        slot: u8,
    ) -> DbResult<Option<PlayerSummaryRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PLAYER_SUMMARY_COLS}
             FROM game.players p
             JOIN game.player_progression prog ON prog.player_id = p.id
             WHERE p.username = $1
               AND p.slot = $2
               AND p.deleted_at IS NULL
             LIMIT 1"
        ))
        .bind(username)
        .bind(i32::from(slot))
        .fetch_optional(pool)
        .await
        .db_ctx("query player summary by slot")?;

        row.map(|row| parse_player_summary_row(&row)).transpose()
    }

    pub async fn find_player_stats_bootstrap_by_id(
        pool: &PgPool,
        id: PlayerId,
    ) -> DbResult<Option<PlayerStatsBootstrapRow>> {
        const PLAYER_STATS_BOOTSTRAP_COLS: &str = "p.id, p.username, p.slot, p.name, p.class_name, p.gender, p.appearance, prog.level, prog.exp_in_level, prog.allocated_str, prog.allocated_vit, prog.allocated_dex, prog.allocated_int, prog.stat_reset_count, prog.playtime_secs, runtime.current_hp, runtime.current_sp, runtime.current_stamina";
        let row = sqlx::query(&format!(
            "SELECT {PLAYER_STATS_BOOTSTRAP_COLS}
             FROM game.players p
             JOIN game.player_progression prog ON prog.player_id = p.id
             LEFT JOIN game.player_runtime_state runtime ON runtime.player_id = p.id
             WHERE p.id = $1
               AND p.deleted_at IS NULL
             LIMIT 1"
        ))
        .bind(i64::from(id))
        .fetch_optional(pool)
        .await
        .db_ctx("query player stats bootstrap by id")?;

        row.map(|row| parse_player_stats_bootstrap_row(&row))
            .transpose()
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
    ) -> DbResult<CreatePlayerOutcome> {
        let mut tx = pool.begin().await.db_ctx("begin create player")?;

        let player_row = sqlx::query(
            "INSERT INTO game.players (username, slot, name, class_name, gender, appearance)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT DO NOTHING
             RETURNING id, username, slot, name, class_name, gender, appearance",
        )
        .bind(username)
        .bind(i32::from(slot))
        .bind(name)
        .bind(DbPlayerClass::from(class).as_ref())
        .bind(DbPlayerGender::from(gender).as_ref())
        .bind(DbAppearance::from(appearance).as_ref())
        .fetch_optional(&mut *tx)
        .await
        .db_ctx("insert player identity")?;

        let Some(player_row) = player_row else {
            return Ok(CreatePlayerOutcome::NameTaken);
        };

        let player_id = player_row.try_get::<i64, _>("id").db_ctx("read id")?;

        sqlx::query(
            "INSERT INTO game.player_progression (
                player_id,
                level
             ) VALUES ($1, 1)",
        )
        .bind(player_id)
        .execute(&mut *tx)
        .await
        .db_ctx("insert player progression")?;

        sqlx::query(
            "INSERT INTO game.player_runtime_state (player_id)
             VALUES ($1)",
        )
        .bind(player_id)
        .execute(&mut *tx)
        .await
        .db_ctx("insert player runtime state")?;

        let row = sqlx::query(&format!(
            "SELECT {PLAYER_SUMMARY_COLS}
             FROM game.players p
             JOIN game.player_progression prog ON prog.player_id = p.id
             WHERE p.id = $1
             LIMIT 1"
        ))
        .bind(player_id)
        .fetch_one(&mut *tx)
        .await
        .db_ctx("fetch created player summary")?;

        tx.commit().await.db_ctx("commit create player")?;

        Ok(CreatePlayerOutcome::Created(parse_player_summary_row(
            &row,
        )?))
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

    pub async fn list_player_runtime_states_for_user(
        pool: &PgPool,
        username: &str,
    ) -> DbResult<Vec<PlayerRuntimeStateRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {PLAYER_RUNTIME_COLS}
             FROM game.player_runtime_state runtime
             JOIN game.players p ON p.id = runtime.player_id
             WHERE p.username = $1
               AND p.deleted_at IS NULL
             ORDER BY p.slot"
        ))
        .bind(username)
        .fetch_all(pool)
        .await
        .db_ctx("query player runtime states")?;

        rows.iter().map(parse_player_runtime_state_row).collect()
    }

    pub async fn find_player_runtime_state_by_player_id(
        pool: &PgPool,
        player_id: PlayerId,
    ) -> DbResult<Option<PlayerRuntimeStateRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PLAYER_RUNTIME_COLS}
             FROM game.player_runtime_state runtime
             JOIN game.players p ON p.id = runtime.player_id
             WHERE runtime.player_id = $1
               AND p.deleted_at IS NULL
             LIMIT 1"
        ))
        .bind(i64::from(player_id))
        .fetch_optional(pool)
        .await
        .db_ctx("query player runtime state by player id")?;

        row.map(|row| parse_player_runtime_state_row(&row))
            .transpose()
    }

    pub async fn save_player_snapshot(
        pool: &PgPool,
        snapshot: &PlayerSnapshot,
    ) -> DbResult<PlayerWriteOutcome> {
        let runtime = &snapshot.runtime;
        let progression = &snapshot.progression;
        let row = sqlx::query(
            "WITH target AS (
                SELECT 1
                FROM game.player_runtime_state runtime
                JOIN game.players p ON p.id = runtime.player_id
                WHERE runtime.player_id = $1
                  AND p.deleted_at IS NULL
            ),
            updated_runtime AS (
                UPDATE game.player_runtime_state runtime
                SET map_key = $3,
                    local_x = $4,
                    local_y = $5,
                    current_hp = $6,
                    current_sp = $7,
                    current_stamina = $8,
                    updated_at = NOW()
                WHERE runtime.player_id = $1
                  AND runtime.runtime_epoch = $2
                RETURNING runtime.player_id
            ),
            updated_progression AS (
                UPDATE game.player_progression prog
                SET playtime_secs = GREATEST(prog.playtime_secs, $9),
                    allocated_str = $10,
                    allocated_vit = $11,
                    allocated_dex = $12,
                    allocated_int = $13,
                    stat_reset_count = $14
                FROM updated_runtime runtime
                WHERE prog.player_id = $1
                  AND runtime.player_id = prog.player_id
                RETURNING prog.player_id
            )
            SELECT
                EXISTS(SELECT 1 FROM target) AS player_exists,
                EXISTS(SELECT 1 FROM updated_progression) AS updated",
        )
        .bind(i64::from(runtime.id))
        .bind(i64::from(runtime.runtime_epoch))
        .bind(&runtime.map_key)
        .bind(runtime.local_pos.x)
        .bind(runtime.local_pos.y)
        .bind(runtime.current_hp)
        .bind(runtime.current_sp)
        .bind(runtime.current_stamina)
        .bind(runtime.playtime.as_secs_i64())
        .bind(progression.core_stat_allocations.allocated_str)
        .bind(progression.core_stat_allocations.allocated_vit)
        .bind(progression.core_stat_allocations.allocated_dex)
        .bind(progression.core_stat_allocations.allocated_int)
        .bind(progression.stat_reset_count)
        .fetch_one(pool)
        .await
        .db_ctx("save player snapshot")?;

        let player_exists = row
            .try_get::<bool, _>("player_exists")
            .db_ctx("read player_exists")?;
        if !player_exists {
            Err(crate::DbError::Invariant(
                "active player should exist when saving player snapshot",
            ))
        } else if row.try_get::<bool, _>("updated").db_ctx("read updated")? {
            Ok(PlayerWriteOutcome::Saved)
        } else {
            Ok(PlayerWriteOutcome::StaleOwner)
        }
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

    pub async fn release_session(
        pool: &PgPool,
        username: &str,
        server_id: &str,
        connection_id: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE game.sessions
             SET server_id = NULL, connection_id = NULL, state = 'AUTHED'
             WHERE username = $1 AND server_id = $2 AND connection_id = $3",
        )
        .bind(username)
        .bind(server_id)
        .bind(connection_id)
        .execute(pool)
        .await
        .db_ctx("release session")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn commit_player_exit(
        pool: &PgPool,
        _exit_kind: PlayerExitKind,
        username: &str,
        server_id: &str,
        connection_id: &str,
        snapshot: &PlayerSnapshot,
    ) -> DbResult<()> {
        let mut tx = pool.begin().await.db_ctx("begin commit player exit")?;
        let runtime = &snapshot.runtime;
        let progression = &snapshot.progression;

        let player_result = sqlx::query(
            "WITH updated_runtime AS (
                 UPDATE game.player_runtime_state runtime
                 SET map_key = $1,
                     local_x = $2,
                     local_y = $3,
                     current_hp = $6,
                     current_sp = $7,
                     current_stamina = $8,
                     runtime_epoch = runtime_epoch + 1,
                     updated_at = NOW()
                 FROM game.players p
                 WHERE runtime.player_id = $4
                   AND p.id = runtime.player_id
                   AND p.deleted_at IS NULL
                   AND runtime_epoch = $5
                 RETURNING runtime.player_id
             )
             UPDATE game.player_progression prog
             SET playtime_secs = GREATEST(prog.playtime_secs, $9),
                 allocated_str = $10,
                 allocated_vit = $11,
                 allocated_dex = $12,
                 allocated_int = $13,
                 stat_reset_count = $14
             FROM updated_runtime runtime
             WHERE prog.player_id = runtime.player_id",
        )
        .bind(&runtime.map_key)
        .bind(runtime.local_pos.x)
        .bind(runtime.local_pos.y)
        .bind(i64::from(runtime.id))
        .bind(i64::from(runtime.runtime_epoch))
        .bind(runtime.current_hp)
        .bind(runtime.current_sp)
        .bind(runtime.current_stamina)
        .bind(runtime.playtime.as_secs_i64())
        .bind(progression.core_stat_allocations.allocated_str)
        .bind(progression.core_stat_allocations.allocated_vit)
        .bind(progression.core_stat_allocations.allocated_dex)
        .bind(progression.core_stat_allocations.allocated_int)
        .bind(progression.stat_reset_count)
        .execute(&mut *tx)
        .await
        .db_ctx("save player runtime state during player exit")?;

        if player_result.rows_affected() != 1 {
            return Err(crate::DbError::Invariant(
                "active player should exist and own the runtime version when committing player exit",
            ));
        }

        let session_result = sqlx::query(
            "UPDATE game.sessions
             SET server_id = NULL,
                 connection_id = NULL,
                 state = 'AUTHED'
             WHERE username = $1 AND server_id = $2 AND connection_id = $3",
        )
        .bind(username)
        .bind(server_id)
        .bind(connection_id)
        .execute(&mut *tx)
        .await
        .db_ctx("release session during player exit")?;

        if session_result.rows_affected() != 1 {
            return Err(crate::DbError::Invariant(
                "active session should exist when committing player exit",
            ));
        }

        tx.commit().await.db_ctx("commit player exit")?;
        Ok(())
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
