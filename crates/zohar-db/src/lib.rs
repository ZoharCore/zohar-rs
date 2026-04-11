//! zohar-db: Database abstraction layer for Zohar.
//!
//! This crate provides a backend-agnostic database interface via traits
//! `AuthDb` and `GameDb`, with backend-specific implementations controlled
//! by feature flags.
//!
//! ## Features
//!
//! - `postgres-backend`: PostgreSQL backend
//!
//! ## Usage
//!
//! ```ignore
//! use zohar_db::{Auth, Game, AuthDb, GameDb};
//!
//! // Open combined for dev mode
//! let (auth_db, game_db) =
//!     zohar_db::postgres_backend::open_combined_db("postgres://localhost/zohar").await?;
//!
//! // Use traits for type-safe access
//! let account = auth_db.accounts().find_by_username("admin").await?;
//! let players = game_db.players().list_summaries_for_user("admin").await?;
//! ```

// =============================================================================
// Feature Gate Validation
// =============================================================================

#[cfg(not(any(feature = "db-auth", feature = "db-game")))]
compile_error!("Enable at least one db feature: 'db-auth' and/or 'db-game'");

#[cfg(not(any(feature = "postgres-backend")))]
compile_error!("At least one backend feature must be enabled. Available: 'postgres-backend'");

// =============================================================================
// Modules
// =============================================================================

#[cfg(feature = "db-game")]
mod db_types;
mod errors;
mod traits;

#[cfg(feature = "postgres-backend")]
pub mod postgres_backend;

// =============================================================================
// Trait Re-exports
// =============================================================================

#[cfg(feature = "db-game")]
pub use errors::parse_enum;
pub use errors::{DbContext, DbError, DbResult, OptionDbExt};
#[cfg(feature = "db-auth")]
pub use traits::{
    // Response types
    AccountRow,
    // View traits
    AccountsView,
    // Bundle traits
    AuthDb,
};
#[cfg(feature = "db-game")]
pub use traits::{
    AcquireSessionResult, CreatePlayerOutcome, GameDb, PlayerCoreStatAllocationRow,
    PlayerRuntimeStateRow, PlayerStatesView, PlayerStatsBootstrapRow, PlayerSummaryRow,
    PlayerWriteOutcome, PlayersView, ProfileRow, ProfilesView, SessionsView,
};

// =============================================================================
// Backend-specific Type Aliases
// =============================================================================

/// Auth database bundle for the currently enabled backend.
#[cfg(all(feature = "postgres-backend", feature = "db-auth"))]
pub type Auth = postgres_backend::PgAuthDb;

/// Game database bundle for the currently enabled backend.
#[cfg(all(feature = "postgres-backend", feature = "db-game"))]
pub type Game = postgres_backend::PgGameDb;
