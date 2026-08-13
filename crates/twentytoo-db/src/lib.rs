//! The PostgreSQL layer for twentytoo's built-in operations.
//!
//! Owns the schema (`migrations/`, embedded via [`MIGRATOR`]) and a typed
//! access layer for the framework-owned tables: users and sessions (auth),
//! groups and membership (groupings), roles and permissions (RBAC, see
//! [`crate::access::load_actor`]), and the append-only audit log.
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
pub mod audit;
pub mod db;
pub mod error;
pub mod groups;
pub mod sessions;
pub mod users;

pub use crate::db::{Db, MIGRATOR};
pub use crate::error::DbError;
