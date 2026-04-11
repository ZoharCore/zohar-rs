//! Backend-agnostic database traits for type-safe access.
//!
//! [`AuthDb`] and [`GameDb`] define the contract for database bundles,
//! with view traits for each domain area. Concrete implementations
//! are provided per backend (e.g., Turso).

use std::future::Future;

use crate::DbResult;
#[cfg(feature = "db-game")]
use zohar_domain::entity::player::{
    PlayerBaseAppearance as DomainAppearanceVariant, PlayerClass as DomainPlayerClass,
    PlayerGender as DomainPlayerGender, PlayerId, PlayerRuntimeEpoch, PlayerSnapshot,
};
#[cfg(feature = "db-game")]
use zohar_domain::{Empire as DomainEmpire, PlayerExitKind};

// =============================================================================
// Response Types (Portable across backends)
// =============================================================================

/// Account credentials returned from auth database.
#[cfg(feature = "db-auth")]
#[derive(Debug, Clone)]
pub struct AccountRow {
    pub username: String,
    pub password_hash: String,
}

/// Account game profile data.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub struct ProfileRow {
    pub username: String,
    pub empire: Option<DomainEmpire>,
    pub delete_code: String,
    pub is_banned: bool,
}

/// Player character data.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCoreStatAllocationRow {
    pub allocated_str: i32,
    pub allocated_vit: i32,
    pub allocated_dex: i32,
    pub allocated_int: i32,
}

/// Player character data.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub struct PlayerSummaryRow {
    pub id: PlayerId,
    pub username: String,
    pub slot: i32,
    pub name: String,
    pub class: DomainPlayerClass,
    pub gender: DomainPlayerGender,
    pub appearance: DomainAppearanceVariant,
    pub level: i32,
    pub playtime_secs: i64,
    pub core_stat_allocations: PlayerCoreStatAllocationRow,
}

/// Player bootstrap data for gameplay systems such as stats.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub struct PlayerStatsBootstrapRow {
    pub id: PlayerId,
    pub username: String,
    pub slot: i32,
    pub name: String,
    pub class: DomainPlayerClass,
    pub gender: DomainPlayerGender,
    pub appearance: DomainAppearanceVariant,
    pub level: i32,
    pub exp_in_level: i64,
    pub core_stat_allocations: PlayerCoreStatAllocationRow,
    pub stat_reset_count: i32,
    pub playtime_secs: i64,
    pub current_hp: Option<i32>,
    pub current_sp: Option<i32>,
    pub current_stamina: Option<i32>,
}

/// Persisted runtime state for a player.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub struct PlayerRuntimeStateRow {
    pub player_id: PlayerId,
    pub map_key: Option<String>,
    pub local_x: Option<f32>,
    pub local_y: Option<f32>,
    pub current_hp: Option<i32>,
    pub current_sp: Option<i32>,
    pub current_stamina: Option<i32>,
    pub runtime_epoch: PlayerRuntimeEpoch,
}

#[cfg(feature = "db-game")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerWriteOutcome {
    Saved,
    StaleOwner,
}

/// Outcome of a player creation attempt.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub enum CreatePlayerOutcome {
    Created(PlayerSummaryRow),
    NameTaken,
}

/// Result of attempting to acquire a session.
#[cfg(feature = "db-game")]
#[derive(Debug, Clone)]
pub enum AcquireSessionResult {
    /// Session acquired successfully
    Acquired,
    /// Session exists on another server and is still active
    AlreadyOnOtherServer { server_id: String },
}

// =============================================================================
// Auth Database Traits
// =============================================================================

/// Auth database bundle trait.
///
/// Provides access to account credentials. Auth server uses this exclusively.
#[cfg(feature = "db-auth")]
pub trait AuthDb: Clone + Send + Sync + 'static {
    type Accounts<'a>: AccountsView
    where
        Self: 'a;

    fn accounts(&self) -> Self::Accounts<'_>;
}

/// View trait for account credential operations.
#[cfg(feature = "db-auth")]
pub trait AccountsView: Send + Sync {
    fn find_by_username(
        &self,
        username: &str,
    ) -> impl Future<Output = DbResult<Option<AccountRow>>> + Send;

    fn update_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> impl Future<Output = DbResult<()>> + Send;
}

// =============================================================================
// Game Database Traits
// =============================================================================

/// Game database bundle trait.
///
/// Provides access to profiles, players, and sessions. Game server uses this.
#[cfg(feature = "db-game")]
pub trait GameDb: Clone + Send + Sync + 'static {
    type Profiles<'a>: ProfilesView
    where
        Self: 'a;
    type Players<'a>: PlayersView
    where
        Self: 'a;
    type PlayerStates<'a>: PlayerStatesView
    where
        Self: 'a;
    type Sessions<'a>: SessionsView
    where
        Self: 'a;

    fn profiles(&self) -> Self::Profiles<'_>;
    fn players(&self) -> Self::Players<'_>;
    fn player_states(&self) -> Self::PlayerStates<'_>;
    fn sessions(&self) -> Self::Sessions<'_>;
}

/// View trait for game profile operations.
#[cfg(feature = "db-game")]
pub trait ProfilesView: Send + Sync {
    fn find_by_username(
        &self,
        username: &str,
    ) -> impl Future<Output = DbResult<Option<ProfileRow>>> + Send;

    fn get_or_create(&self, username: &str) -> impl Future<Output = DbResult<ProfileRow>> + Send;

    fn update_empire(
        &self,
        username: &str,
        empire: DomainEmpire,
    ) -> impl Future<Output = DbResult<()>> + Send;

    fn get_delete_code(
        &self,
        username: &str,
    ) -> impl Future<Output = DbResult<Option<String>>> + Send;
}

/// View trait for player character operations.
#[cfg(feature = "db-game")]
pub trait PlayersView: Send + Sync {
    fn list_summaries_for_user(
        &self,
        username: &str,
    ) -> impl Future<Output = DbResult<Vec<PlayerSummaryRow>>> + Send;

    fn find_summary_by_slot(
        &self,
        username: &str,
        slot: u8,
    ) -> impl Future<Output = DbResult<Option<PlayerSummaryRow>>> + Send;

    fn find_stats_bootstrap_by_id(
        &self,
        id: PlayerId,
    ) -> impl Future<Output = DbResult<Option<PlayerStatsBootstrapRow>>> + Send;

    fn create(
        &self,
        username: &str,
        slot: u8,
        name: &str,
        class: DomainPlayerClass,
        gender: DomainPlayerGender,
        appearance: DomainAppearanceVariant,
    ) -> impl Future<Output = DbResult<CreatePlayerOutcome>> + Send;

    fn delete_with_code(
        &self,
        username: &str,
        slot: u8,
        delete_code: &str,
    ) -> impl Future<Output = DbResult<bool>> + Send;
}

/// View trait for persisted player runtime state operations.
#[cfg(feature = "db-game")]
pub trait PlayerStatesView: Send + Sync {
    fn list_for_user(
        &self,
        username: &str,
    ) -> impl Future<Output = DbResult<Vec<PlayerRuntimeStateRow>>> + Send;

    fn find_by_player_id(
        &self,
        player_id: PlayerId,
    ) -> impl Future<Output = DbResult<Option<PlayerRuntimeStateRow>>> + Send;

    fn save_player_snapshot(
        &self,
        snapshot: &PlayerSnapshot,
    ) -> impl Future<Output = DbResult<PlayerWriteOutcome>> + Send;
}

/// View trait for session management operations.
#[cfg(feature = "db-game")]
pub trait SessionsView: Send + Sync {
    fn acquire(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> impl Future<Output = DbResult<AcquireSessionResult>> + Send;

    fn resume_with_token(
        &self,
        username: &str,
        login_token: u32,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> impl Future<Output = DbResult<bool>> + Send;

    fn validate_login_token(
        &self,
        username: &str,
        login_token: u32,
        idle_ttl_secs: i64,
        peer_ip: &str,
    ) -> impl Future<Output = DbResult<bool>> + Send;

    fn set_login_token(
        &self,
        username: &str,
        login_token: u32,
    ) -> impl Future<Output = DbResult<()>> + Send;

    fn mark_stale(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
        stale_threshold_secs: i64,
    ) -> impl Future<Output = DbResult<()>> + Send;

    fn release(
        &self,
        username: &str,
        server_id: &str,
        connection_id: &str,
    ) -> impl Future<Output = DbResult<bool>> + Send;

    fn commit_player_exit(
        &self,
        exit_kind: PlayerExitKind,
        username: &str,
        server_id: &str,
        connection_id: &str,
        snapshot: &PlayerSnapshot,
    ) -> impl Future<Output = DbResult<()>> + Send;

    fn update_heartbeat(&self, username: &str) -> impl Future<Output = DbResult<()>> + Send;
}
