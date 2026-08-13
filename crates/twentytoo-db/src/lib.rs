//! The PostgreSQL layer for twentytoo's built-in operations.
//!
//! Owns the schema (`migrations/`, embedded via [`MIGRATOR`]) and a typed
//! access layer for the core framework tables: users and sessions (auth),
//! teams and membership (groupings), roles and permissions (RBAC, see
//! [`crate::access::load_actor`]).
//!
//! Queries are runtime-bound (`sqlx::query_as`), so the crate compiles and
//! its unit tests run without a live database; integration tests live in
//! `tests/db.rs` and run against `DATABASE_URL`.
//!
//! The generic per-resource `SqlxAdapter` for user entities is a later
//! slice (`03-data-adapter.md` §10); this crate covers only the
//! framework-owned tables (`01` §10.5).

#![warn(missing_docs)]

pub mod access;
pub mod error;
pub mod sessions;
pub mod teams;
pub mod users;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub use crate::error::DbError;

/// The embedded schema; run it with [`Db::migrate`]. Migrations apply in
/// filename order and are recorded in `_sqlx_migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// A PostgreSQL connection pool with the built-in schema on demand.
///
/// Clone is cheap (a shared pool); hand one `Db` to the whole application.
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Connect to `url` with a small default pool (5 connections).
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;
        return Ok(Self { pool });
    }

    /// The underlying pool, for transactions and raw queries.
    pub fn pool(&self) -> &PgPool {
        return &self.pool;
    }

    /// Apply all pending migrations in order (`01` §10.5).
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        return MIGRATOR.run(&self.pool).await;
    }
}
