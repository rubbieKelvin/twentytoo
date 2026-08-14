//! Login-token queries: the typed access layer over the `login_tokens`
//! table.
//!
//! The client holds a random step token; the table stores only its hash,
//! matching the sessions-table pattern (a leaked table yields no usable
//! credentials). Tokens are single-use: `used_at` is stamped exactly once,
//! and the `expires_at > now()` filter keeps expired tokens invisible to
//! every lookup and mutation. The row shape lives in
//! [`crate::entities::login_token`].

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::Db;
use crate::entities::{LoginPurpose, LoginToken};
use crate::error::DbError;

impl Db {
    /// Record a step token. `user_id` is `None` when the email step has not
    /// yet resolved an account; `code_hash` is `Some` when email
    /// confirmation is on and the token carries the emailed code's hash.
    pub async fn create_login_token(
        &self,
        token_hash: &str,
        email: &str,
        user_id: Option<&Uuid>,
        purpose: LoginPurpose,
        code_hash: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO login_tokens (token_hash, email, user_id, purpose, code_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(token_hash)
        .bind(email)
        .bind(user_id)
        .bind(purpose.as_str())
        .bind(code_hash)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// Load an unconsumed, unexpired token by hash; `None` for unknown,
    /// already-used, or expired tokens.
    pub async fn get_login_token(&self, token_hash: &str) -> Result<Option<LoginToken>, DbError> {
        let row = sqlx::query_as::<_, LoginToken>(
            "SELECT token_hash, email, user_id, purpose, code_hash, attempts, used_at,
                    expires_at, created_at
             FROM login_tokens
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Consume a token atomically (stamp `used_at`), so a racing second
    /// consumer sees it as gone. `true` iff exactly one row was consumed.
    pub async fn consume_login_token(&self, token_hash: &str) -> Result<bool, DbError> {
        let affected = sqlx::query(
            "UPDATE login_tokens SET used_at = now()
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()",
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?
        .rows_affected();
        return Ok(affected == 1);
    }

    /// Increment the wrong-attempt counter and return the new count.
    /// [`DbError::NotFound`] when the token is unknown, already used, or
    /// expired.
    pub async fn bump_login_token_attempts(&self, token_hash: &str) -> Result<u64, DbError> {
        let attempts: Option<i32> = sqlx::query_scalar::<_, i32>(
            "UPDATE login_tokens SET attempts = attempts + 1
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
             RETURNING attempts",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        return match attempts {
            Some(n) => Ok(n as u64),
            None => Err(DbError::NotFound),
        };
    }

    /// Delete every expired token; returns the number removed.
    pub async fn delete_expired_login_tokens(&self) -> Result<u64, DbError> {
        let affected = sqlx::query("DELETE FROM login_tokens WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?
            .rows_affected();
        return Ok(affected);
    }
}
