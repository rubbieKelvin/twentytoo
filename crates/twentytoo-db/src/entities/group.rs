//! The group row.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// One row of `groups`.
#[derive(Clone, Debug, FromRow)]
pub struct Group {
    /// Stable id.
    pub id: Uuid,
    /// Display name.
    pub name: String,
    /// Unique URL- and reference-safe key.
    pub slug: String,
    /// Row creation time.
    pub created_at: DateTime<Utc>,
}
