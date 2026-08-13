//! The permission row.

use sqlx::FromRow;
use uuid::Uuid;

/// One row of `permissions`.
#[derive(Clone, Debug, FromRow)]
pub struct Permission {
    /// Stable id.
    pub id: Uuid,
    /// The `resource.action` code, e.g. `"stores.view"`.
    pub code: String,
    /// Human explanation of what the permission allows.
    pub description: String,
}
