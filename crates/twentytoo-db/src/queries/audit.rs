//! Audit queries: the append-only trail of mutations and actions
//! (`00-init` §5.5).
//!
//! Every entry records the actor (id + email snapshots), the affected
//! resource + record, the before/after state, and the request IP. The
//! access layer only inserts and selects — entries are immutable; retention
//! is a storage-layer concern, not an application one. The write shape
//! lives in [`crate::entities::audit`].

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use twentytoo_core::{AuditAction, AuditEntry};
use uuid::Uuid;

use crate::Db;
use crate::entities::NewAuditEntry;
use crate::error::DbError;

/// One stored row, before the `action` text is mapped to [`AuditAction`].
#[derive(FromRow)]
struct AuditRow {
    id: Uuid,
    actor_id: String,
    actor_email: String,
    action: String,
    resource: String,
    resource_id: String,
    before: Option<Value>,
    after: Option<Value>,
    ip: Option<String>,
    created_at: DateTime<Utc>,
}

/// The stored text for an [`AuditAction`] (mirrors the `audit_log.action`
/// `CHECK` constraint).
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

/// Parse a stored `action` text back into an [`AuditAction`]. The column's
/// `CHECK` constraint makes `None` unreachable in practice.
fn action_from_str(s: &str) -> Option<AuditAction> {
    return match s {
        "create" => Some(AuditAction::Create),
        "update" => Some(AuditAction::Update),
        "delete" => Some(AuditAction::Delete),
        "execute" => Some(AuditAction::Execute),
        "login" => Some(AuditAction::Login),
        "logout" => Some(AuditAction::Logout),
        "impersonate" => Some(AuditAction::Impersonate),
        _ => None,
    };
}

impl AuditRow {
    /// Map the stored `action` text into an [`AuditEntry`].
    fn into_entry(self) -> Result<AuditEntry, DbError> {
        let action = action_from_str(&self.action).ok_or_else(|| {
            return DbError::Internal(sqlx::Error::Decode(
                "audit action outside the CHECK set".into(),
            ));
        })?;
        return Ok(AuditEntry {
            id: self.id,
            actor_id: self.actor_id,
            actor_email: self.actor_email,
            action,
            resource_key: self.resource,
            record_id: self.resource_id,
            before: self.before,
            after: self.after,
            ip: self.ip,
            created_at: self.created_at,
        });
    }
}

impl Db {
    /// Append one audit entry. Returns the stored row with its id and
    /// timestamp. Entries are append-only: there is no update or delete.
    pub async fn record_audit(&self, entry: &NewAuditEntry) -> Result<AuditEntry, DbError> {
        let row = sqlx::query_as::<_, AuditRow>(
            "INSERT INTO audit_log
                 (actor_id, actor_email, action, resource, resource_id, before, after, ip)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, actor_id, actor_email, action, resource, resource_id,
                       before, after, ip, created_at",
        )
        .bind(&entry.actor_id)
        .bind(&entry.actor_email)
        .bind(action_to_str(&entry.action))
        .bind(&entry.resource)
        .bind(&entry.resource_id)
        .bind(&entry.before)
        .bind(&entry.after)
        .bind(&entry.ip)
        .fetch_one(&self.pool)
        .await?;
        return row.into_entry();
    }

    /// The most recent `limit` audit entries across every resource, newest
    /// first.
    pub async fn list_audit(&self, limit: i64) -> Result<Vec<AuditEntry>, DbError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, actor_id, actor_email, action, resource, resource_id,
                    before, after, ip, created_at

             FROM audit_log
             ORDER BY created_at DESC, id DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(AuditRow::into_entry).collect();
    }

    /// The audit history for one record, newest first.
    pub async fn list_audit_for_record(
        &self,
        resource: &str,
        resource_id: &str,
    ) -> Result<Vec<AuditEntry>, DbError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, actor_id, actor_email, action, resource, resource_id,
                    before, after, ip, created_at
             FROM audit_log
             WHERE resource = $1 AND resource_id = $2
             ORDER BY created_at DESC, id DESC",
        )
        .bind(resource)
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(AuditRow::into_entry).collect();
    }

    /// The audit history for one actor, newest first.
    pub async fn list_audit_for_actor(&self, actor_id: &str) -> Result<Vec<AuditEntry>, DbError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, actor_id, actor_email, action, resource, resource_id,
                    before, after, ip, created_at
             FROM audit_log
             WHERE actor_id = $1
             ORDER BY created_at DESC, id DESC",
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await?;
        return rows.into_iter().map(AuditRow::into_entry).collect();
    }
}
