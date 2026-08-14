//! The fields needed to write one domain event into the audit trail.
//!
//! [`crate::Db::record_audit`] maps these onto the `inapp_events` envelope
//! (`00` §6.5): `type` becomes `"{resource}.{action}"`, `actor` a
//! `"user"` envelope with the id/email snapshot, `target` a
//! `{"type": resource, "properties": {"id": resource_id}}` envelope,
//! `properties` the `{"before": …, "after": …}` payload, and `context`
//! the `{"client_ip": …}` request metadata.

use serde_json::Value;
use twentytoo_core::AuditAction;

/// The fields needed to write one audit event.
#[derive(Clone, Debug)]
pub struct NewAuditEntry {
    /// Acting user id (snapshot — survives user deletion).
    pub actor_id: String,
    /// Acting user email (snapshot).
    pub actor_email: String,
    /// What happened.
    pub action: AuditAction,
    /// Resource key (`"stores"`, `"orders"`, …).
    pub resource: String,
    /// The affected record's id.
    pub resource_id: String,
    /// Record state before the mutation.
    pub before: Option<Value>,
    /// Record state after the mutation.
    pub after: Option<Value>,
    /// Client IP, when known.
    pub ip: Option<String>,
}
