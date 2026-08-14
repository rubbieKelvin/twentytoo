//! Audit queries: the append-only trail of domain events (`00` §6.5).
//!
//! Events live in `inapp_events` — the canonical envelope (`type`, `actor`,
//! `target`, `properties`, `context`). The access layer only inserts and
//! selects — events are immutable; retention is a storage-layer concern,
//! not an application one. Scoped reads filter the envelopes directly
//! (`target` for per-record history, `actor` for per-actor history); a
//! dedicated audit-junction table for permissioned reads is a later
//! slice. The write shape lives in [`crate::entities::audit`].

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sqlx::FromRow;
use twentytoo_core::{AuditAction, AuditEvent, EventResource};
use uuid::Uuid;

use crate::Db;
use crate::entities::NewAuditEntry;
use crate::error::DbError;

/// One `inapp_events` row, before the jsonb columns are mapped to the typed
/// [`AuditEvent`] envelope.
#[derive(FromRow)]
struct EventRow {
    id: Uuid,
    timestamp: DateTime<Utc>,
    #[sqlx(rename = "type")]
    event_type: String,
    actor: Value,
    target: Value,
    properties: Value,
    context: Value,
}

/// The stored text for an [`AuditAction`] (the `resource.action` suffix).
fn action_to_str(action: &AuditAction) -> &'static str {
    return match action {
        AuditAction::Create => "create",
        AuditAction::Update => "update",
        AuditAction::Delete => "delete",
        AuditAction::Execute => "execute",
        AuditAction::Login => "login",
        AuditAction::Logout => "logout",
        AuditAction::Impersonate => "impersonate",
    };
}

/// Map a stored jsonb envelope (`{"type": …, "properties": …}`) into an
/// [`EventResource`]. The writer guarantees the shape, so a mismatch is
/// a data-layer bug, not a parse error to recover from.
fn resource_from_json(v: &Value) -> Result<EventResource, DbError> {
    let kind = v.get("type").and_then(Value::as_str).ok_or_else(|| {
        return DbError::Internal(sqlx::Error::Decode(
            "inapp_events envelope missing 'type'".into(),
        ));
    })?;
    let properties = v.get("properties").cloned().ok_or_else(|| {
        return DbError::Internal(sqlx::Error::Decode(
            "inapp_events envelope missing 'properties'".into(),
        ));
    })?;
    return Ok(EventResource {
        kind: kind.to_string(),
        properties,
    });
}

impl EventRow {
    /// Map the stored row into an [`AuditEvent`].
    fn into_event(self) -> Result<AuditEvent, DbError> {
        let actor = resource_from_json(&self.actor)?;
        let target = resource_from_json(&self.target)?;
        return Ok(AuditEvent {
            id: self.id,
            timestamp: self.timestamp,
            event_type: self.event_type,
            actor,
            target,
            properties: self.properties,
            context: self.context,
        });
    }
}

impl Db {
    /// Append one event to the audit trail and return the stored event
    /// (with its id and timestamp). Append-only: there is no update or
    /// delete.
    pub async fn record_audit(&self, entry: &NewAuditEntry) -> Result<AuditEvent, DbError> {
        let event_type = format!("{}.{}", entry.resource, action_to_str(&entry.action));
        let actor = json!({
            "type": "user",
            "properties": {"id": entry.actor_id, "email": entry.actor_email},
        });
        let target = json!({
            "type": entry.resource,
            "properties": {"id": entry.resource_id},
        });
        let mut properties = Map::new();
        if let Some(before) = &entry.before {
            properties.insert("before".to_string(), before.clone());
        }
        if let Some(after) = &entry.after {
            properties.insert("after".to_string(), after.clone());
        }
        let context = match &entry.ip {
            Some(ip) => json!({"client_ip": ip}),
            None => json!({}),
        };

        let row = sqlx::query_as::<_, EventRow>(
            "INSERT INTO inapp_events (type, actor, target, properties, context)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, timestamp, type, actor, target, properties, context",
        )
        .bind(&event_type)
        .bind(&actor)
        .bind(&target)
        .bind(Value::Object(properties))
        .bind(&context)
        .fetch_one(&self.pool)
        .await?;
        return row.into_event();
    }

    /// The most recent `limit` audit events across every resource,
    /// newest first.
    pub async fn list_audit(&self, limit: i64) -> Result<Vec<AuditEvent>, DbError> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT id, timestamp, type, actor, target, properties, context
             FROM inapp_events
             ORDER BY timestamp DESC, id DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(EventRow::into_event).collect();
    }

    /// The audit history for one record, newest first.
    pub async fn list_audit_for_record(
        &self,
        resource: &str,
        resource_id: &str,
    ) -> Result<Vec<AuditEvent>, DbError> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT id, timestamp, type, actor, target, properties, context
             FROM inapp_events
             WHERE target ->> 'type' = $1 AND target -> 'properties' ->> 'id' = $2
             ORDER BY timestamp DESC, id DESC",
        )
        .bind(resource)
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(EventRow::into_event).collect();
    }

    /// The audit history for one actor, newest first.
    pub async fn list_audit_for_actor(&self, actor_id: &str) -> Result<Vec<AuditEvent>, DbError> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT id, timestamp, type, actor, target, properties, context
             FROM inapp_events
             WHERE actor -> 'properties' ->> 'id' = $1
             ORDER BY timestamp DESC, id DESC",
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(EventRow::into_event).collect();
    }
}
