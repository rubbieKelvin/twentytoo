//! The connection pool and the embedded schema.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// The embedded schema; run it with [`Db::migrate`]. Migrations apply in
/// filename order and are recorded in `_sqlx_migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// A PostgreSQL connection pool with the built-in schema on demand.
///
/// Clone is cheap (a shared pool); hand one `Db` to the whole application.
#[derive(Clone)]
pub struct Db {
    pub(crate) pool: PgPool,
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

    /// Apply all pending migrations in order (`00` §9).
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        return MIGRATOR.run(&self.pool).await;
    }
}
