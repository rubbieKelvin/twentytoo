//! Session queries: the typed access layer over the `sessions` table.
//!
//! The client holds a random token; the table stores only its hash, so a
//! database leak never yields usable credentials. Callers hash the token
//! (e.g. SHA-256 hex) before calling these methods.
//!
//! Tracking is deliberately wide: [`SessionInfo`] carries the common
//! request facts as optional fields plus an open `metadata` JSON object for
//! anything a deployment wants to record. The row shapes live in
//! [`crate::entities::session`].

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::Db;
use crate::entities::{Session, SessionInfo};
use crate::error::DbError;

impl Db {
    /// Create a session for `user_id`, optionally scoped to a group, with
    /// the optional tracking facts in `info`.
    pub async fn create_session(
        &self,
        token_hash: &str,
        user_id: &Uuid,
        group_id: Option<&Uuid>,
        expires_at: DateTime<Utc>,
        info: &SessionInfo,
    ) -> Result<Session, DbError> {
        let row = sqlx::query_as::<_, Session>(
            "INSERT INTO sessions
                 (token_hash, user_id, group_id, expires_at, user_agent, ip,
                  referrer, accept_language, device, os, browser, metadata)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             RETURNING token_hash, user_id, group_id, created_at, expires_at,
                       last_seen_at, user_agent, ip, referrer, accept_language,
                       device, os, browser, metadata",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(group_id)
        .bind(expires_at)
        .bind(info.user_agent.as_deref())
        .bind(info.ip.as_deref())
        .bind(info.referrer.as_deref())
        .bind(info.accept_language.as_deref())
        .bind(info.device.as_deref())
        .bind(info.os.as_deref())
        .bind(info.browser.as_deref())
        .bind(&info.metadata)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Load a session by token hash; `None` for unknown or already-expired
    /// tokens.
    pub async fn get_session(&self, token_hash: &str) -> Result<Option<Session>, DbError> {
        let row = sqlx::query_as::<_, Session>(
            "SELECT token_hash, user_id, group_id, created_at, expires_at,
                    last_seen_at, user_agent, ip, referrer, accept_language,
                    device, os, browser, metadata
             FROM sessions
             WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Mark the session active now. [`DbError::NotFound`] when the token is
    /// unknown or already expired.
    pub async fn touch_session(&self, token_hash: &str) -> Result<(), DbError> {
        let affected =
            sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&self.pool)
                .await?
                .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound);
        }
        return Ok(());
    }

    /// Delete one session (logout). Deleting a token that is already gone
    /// is a no-op, not an error.
    pub async fn delete_session(&self, token_hash: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        return Ok(());
    }

    /// Delete every expired session; returns the number removed.
    pub async fn delete_expired_sessions(&self) -> Result<u64, DbError> {
        let affected = sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?
            .rows_affected();
        return Ok(affected);
    }
}
