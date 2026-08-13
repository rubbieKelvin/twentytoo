//! Audit entries: who did what to which record, when.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The kind of audited event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditAction {
    /// Record created.
    Create,
    /// Record updated.
    Update,
    /// Record deleted.
    Delete,
    /// A custom action ran.
    Execute,
    /// User signed in.
    Login,
    /// User signed out.
    Logout,
    /// Session impersonated another actor.
    Impersonate,
}

/// One audit trail row.
///
/// Records the actor, timestamp, resource + record id, the before/after
/// diff for mutations, and the request's IP/session anchor.
#[derive(Clone, Debug)]
pub struct AuditEntry {
    /// Entry id.
    pub id: Uuid,
    /// Acting user id.
    pub actor_id: String,
    /// Acting user email.
    pub actor_email: String,
    /// What happened.
    pub action: AuditAction,
    /// Resource key (`"stores"`, `"orders"`, …).
    pub resource_key: String,
    /// The affected record's id.
    pub record_id: String,
    /// Record state before the mutation.
    pub before: Option<serde_json::Value>,
    /// Record state after the mutation.
    pub after: Option<serde_json::Value>,
    /// Client IP, when known.
    pub ip: Option<String>,
    /// When the event happened.
    pub created_at: DateTime<Utc>,
}
