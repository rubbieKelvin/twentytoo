//! The role row.

use sqlx::FromRow;
use uuid::Uuid;

/// One row of `roles`.
#[derive(Clone, Debug, FromRow)]
pub struct Role {
    /// Stable id.
    pub id: Uuid,
    /// Stable key, e.g. `"admin"`.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Human explanation of what the role bundles.
    pub description: String,
}
