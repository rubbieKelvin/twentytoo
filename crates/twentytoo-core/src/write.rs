//! The write surface: mutations and their context.

use crate::actor::Actor;

/// An opaque optimistic-concurrency token: DB row version / API etag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version(pub String);

/// One write operation, as used by `apply_mutations` (import wizard, bulk
/// actions).
#[derive(Clone, Debug)]
pub enum Mutation<Id> {
    /// Insert a new record.
    Create {
        /// Full record payload.
        data: serde_json::Value,
    },
    /// Merge a patch into an existing record.
    Update {
        /// Record id.
        id: Id,
        /// Partial payload.
        patch: serde_json::Value,
    },
    /// Remove a record.
    Delete {
        /// Record id.
        id: Id,
    },
    /// Create-or-update by id (the common CSV re-import shape).
    Upsert {
        /// Record id.
        id: Id,
        /// Full record payload.
        data: serde_json::Value,
    },
}

/// Everything the engine knows about a write, handed to the adapter.
///
/// Most adapters ignore `actor`; `expected_version` and `idempotency_key`
/// give the "two agents processed the same record" case a real answer.
#[derive(Clone, Debug)]
pub struct WriteContext<'a> {
    /// Optimistic concurrency: adapter compares and fails with `Conflict`.
    pub expected_version: Option<Version>,
    /// Idempotency key: HTTP `Idempotency-Key` header, an idempotency
    /// column, or a unique constraint. Import retries use this.
    pub idempotency_key: Option<&'a str>,
    /// Escape hatch for the rare source that authenticates per-user.
    pub actor: Option<&'a Actor>,
}
