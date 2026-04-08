//! PostgreSQL implementations of AuthDb and GameDb bundles.

use sqlx::PgPool;

use super::queries;
#[cfg(feature = "db-auth")]
use crate::traits::{AccountRow, AccountsView, AuthDb};
#[cfg(feature = "db-game")]
use crate::traits::{
    AcquireSessionResult, CreatePlayerOutcome, GameDb, PlayerRow, PlayersView, ProfileRow,
    ProfilesView, RuntimeStateSaveOutcome, SessionsView,
};
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
use zohar_domain::entity::player::PlayerId;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::PlayerRuntimeSnapshot;

use crate::DbResult;

#[cfg(feature = "db-auth")]
#[derive(Clone)]
#[repr(transparent)]
pub struct PgAuthDb {
    pool: PgPool,
}

#[cfg(feature = "db-auth")]
impl PgAuthDb {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "db-auth")]
impl AuthDb for PgAuthDb {
    type Accounts<'a> = PgAccountsView<'a>;

    #[inline]
    fn accounts(&self) -> Self::Accounts<'_> {
        PgAccountsView { pool: &self.pool }
    }
}

#[cfg(feature = "db-auth")]
pub struct PgAccountsView<'a> {
    pool: &'a PgPool,
}

#[cfg(feature = "db-auth")]
impl AccountsView for PgAccountsView<'_> {
    async fn find_by_username(&self, username: &str) -> DbResult<Option<AccountRow>> {
        queries::auth::find_account_by_username(self.pool, username).await
    }

    async fn update_password(&self, username: &str, password_hash: &str) -> DbResult<()> {
        queries::auth::update_password(self.pool, username, password_hash).await
    }
}

#[cfg(feature = "db-game")]
#[derive(Clone)]
#[repr(transparent)]
pub struct PgGameDb {
    pool: PgPool,
}

#[cfg(feature = "db-game")]
impl PgGameDb {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(feature = "db-game")]
impl GameDb for PgGameDb {
    type Profiles<'a> = PgProfilesView<'a>;
    type Players<'a> = PgPlayersView<'a>;
    type Sessions<'a> = PgSessionsView<'a>;

    #[inline]
    fn profiles(&self) -> Self::Profiles<'_> {
        PgProfilesView { pool: &self.pool }
    }

    #[inline]
    fn players(&self) -> Self::Players<'_> {
        PgPlayersView { pool: &self.pool }
    }

    #[inline]
    fn sessions(&self) -> Self::Sessions<'_> {
        PgSessionsView { pool: &self.pool }
    }
}

#[cfg(feature = "db-game")]
pub struct PgProfilesView<'a> {
    pool: &'a PgPool,
}

#[cfg(feature = "db-game")]
impl ProfilesView for PgProfilesView<'_> {
    async fn find_by_username(&self, username: &str) -> DbResult<Option<ProfileRow>> {
        queries::game::find_profile_by_username(self.pool, username).await
    }

    async fn get_or_create(&self, username: &str) -> DbResult<ProfileRow> {
        queries::game::get_or_create_profile(self.pool, username).await
    }

    async fn update_empire(&self, username: &str, empire: DomainEmpire) -> DbResult<()> {
        queries::game::update_profile_empire(self.pool, username, empire).await
    }

    async fn get_delete_code(&self, username: &str) -> DbResult<Option<String>> {
        queries::game::get_delete_code(self.pool, username).await
    }
}

#[cfg(feature = "db-game")]
pub struct PgPlayersView<'a> {
    pool: &'a PgPool,
}

#[cfg(feature = "db-game")]
impl PlayersView for PgPlayersView<'_> {
    async fn list_for_user(&self, username: &str) -> DbResult<Vec<PlayerRow>> {
        queries::game::list_players_for_user(self.pool, username).await
    }

    async fn find_by_slot(&self, username: &str, slot: u8) -> DbResult<Option<PlayerRow>> {
        queries::game::find_player_by_slot(self.pool, username, slot).await
    }

    async fn find_by_id(&self, id: PlayerId) -> DbResult<Option<PlayerRow>> {
        queries::game::find_player_by_id(self.pool, id).await
    }

    async fn create(
        &self,
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
        queries::game::create_player(
            self.pool, username, slot, name, class, gender, appearance, stat_str, stat_vit,
            stat_dex, stat_int,
        )
        .await
    }

    async fn delete_with_code(
        &self,
        username: &str,
        slot: u8,
        delete_code: &str,
    ) -> DbResult<bool> {
        queries::game::delete_player_with_code(self.pool, username, slot, delete_code).await
    }

    async fn save_runtime_state(
        &self,
        snapshot: &PlayerRuntimeSnapshot,
    ) -> DbResult<RuntimeStateSaveOutcome> {
        queries::game::save_player_runtime_state(self.pool, snapshot).await
    }
}

#[cfg(feature = "db-game")]
pub struct PgSessionsView<'a> {
    pool: &'a PgPool,
}

#[cfg(feature = "db-game")]
impl SessionsView for PgSessionsView<'_> {
    async fn acquire(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> DbResult<AcquireSessionResult> {
        queries::game::acquire_session(
            self.pool,
            username,
            server_id,
            connection_id,
            stale_threshold_secs,
        )
        .await
    }

    async fn resume_with_token(
        &self,
        username: &str,
        login_token: u32,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> DbResult<bool> {
        queries::game::resume_session_with_token(
            self.pool,
            username,
            login_token,
            server_id,
            connection_id,
            stale_threshold_secs,
            idle_ttl_secs,
            peer_ip,
        )
        .await
    }

    async fn set_login_token(&self, username: &str, login_token: u32) -> DbResult<()> {
        queries::game::set_session_login_token(self.pool, username, login_token).await
    }

    async fn validate_login_token(
        &self,
        username: &str,
        login_token: u32,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> DbResult<bool> {
        queries::game::validate_login_token(
            self.pool,
            username,
            login_token,
            idle_ttl_secs,
            peer_ip,
        )
        .await
    }

    async fn mark_stale(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> DbResult<()> {
        queries::game::mark_session_stale(
            self.pool,
            username,
            server_id,
            connection_id,
            stale_threshold_secs,
        )
        .await
    }

    async fn release(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
    ) -> DbResult<bool> {
        queries::game::release_session(self.pool, username, server_id, connection_id).await
    }

    async fn commit_player_exit(
        &self,
        exit_kind: PlayerExitKind,
        username: &str,
        server_id: &str,
        connection_id: &str,
        snapshot: &PlayerRuntimeSnapshot,
    ) -> DbResult<()> {
        queries::game::commit_player_exit(
            self.pool,
            exit_kind,
            username,
            server_id,
            connection_id,
            snapshot,
        )
        .await
    }

    async fn update_heartbeat(&self, username: &str) -> DbResult<()> {
        queries::game::update_session_heartbeat(self.pool, username).await
    }
}
