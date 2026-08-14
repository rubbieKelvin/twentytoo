//! Domain events: the append-only audit trail's canonical envelope
//! (the in-app event shape, `00` §6.5).
//!
//! Every event is a `resource.action` discriminator with point-in-time
//! snapshots: `actor` and `target` are [`EventResource`] envelopes
//! (`{"type": "<kind>", "properties": {…}}`), so history survives actor
//! deletion and record renames. [`AuditAction`] is the closed,
//! writer-side union the `type` suffix draws from.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The kind of audited event (the closed, writer-side union).
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

/// A point-in-time snapshot of a resource, embedded in events as
/// `{"type": "<kind>", "properties": {…}}`.
///
/// The snapshot is fixed at emission time: entries keep rendering
/// correctly after the live resource is renamed or deleted.
#[derive(Clone, Debug, PartialEq)]
pub struct EventResource {
    /// Resource kind: `"user"`, `"system"`, `"stores"`, …
    pub kind: String,
    /// Scalar snapshot subset of the resource's fields.
    pub properties: serde_json::Value,
}

/// One append-only domain event (the `inapp_events` envelope).
///
/// Records what happened (`event_type`), who caused it (`actor`), the
/// subject it is about (`target`), the type-specific payload
/// (`properties`), and request metadata (`context`).
#[derive(Clone, Debug)]
pub struct AuditEvent {
    /// Event id.
    pub id: Uuid,
    /// When the event was emitted.
    pub timestamp: DateTime<Utc>,
    /// The `resource.action` discriminator: `"stores.created"`,
    /// `"users.login"`, …
    pub event_type: String,
    /// Who caused the change: a `"user"` (or `"system"`) envelope with
    /// an id/email snapshot.
    pub actor: EventResource,
    /// The primary subject of the event: a `{"type": <resource>,
    /// "properties": {"id": …}}` envelope.
    pub target: EventResource,
    /// Type-specific payload: `{"before": …, "after": …}` record state
    /// for mutations.
    pub properties: serde_json::Value,
    /// Request metadata: `{"client_ip": "…"}` when known.
    pub context: serde_json::Value,
}
