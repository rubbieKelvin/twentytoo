//! Sessions: the server-side session store (`01` §10.6).
//!
//! The client holds a random token; the table stores only its hash, so a
//! database leak never yields usable credentials. Callers hash the token
//! (e.g. SHA-256 hex) before calling these methods.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::Db;
use crate::error::DbError;

/// One row of `sessions`.
#[derive(Clone, Debug, FromRow)]
pub struct Session {
    /// Hash of the session token, not the token itself.
    pub token_hash: String,
    /// The user the session belongs to.
    pub user_id: Uuid,
    /// The team this session acts within, if any.
    pub team_id: Option<Uuid>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Sessions past this point are invalid.
    pub expires_at: DateTime<Utc>,
    /// Last activity time (refreshed by [`Db::touch_session`]).
    pub last_seen_at: Option<DateTime<Utc>>,
    /// The client's user-agent at creation.
    pub user_agent: Option<String>,
    /// The client's address at creation, as a string.
    pub ip: Option<String>,
}

impl Db {
    /// Create a session for `user_id`, optionally scoped to a team.
    pub async fn create_session(
        &self,
        token_hash: &str,
        user_id: &Uuid,
        team_id: Option<&Uuid>,
        expires_at: DateTime<Utc>,
        user_agent: Option<&str>,
        ip: Option<&str>,
    ) -> Result<Session, DbError> {
        let row = sqlx::query_as::<_, Session>(
            "INSERT INTO sessions (token_hash, user_id, team_id, expires_at, user_agent, ip)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING token_hash, user_id, team_id, created_at, expires_at,
                       last_seen_at, user_agent, ip",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(team_id)
        .bind(expires_at)
        .bind(user_agent)
        .bind(ip)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Load a session by token hash; `None` for unknown or already-expired
    /// tokens.
    pub async fn get_session(&self, token_hash: &str) -> Result<Option<Session>, DbError> {
        let row = sqlx::query_as::<_, Session>(
            "SELECT token_hash, user_id, team_id, created_at, expires_at,
                    last_seen_at, user_agent, ip
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
