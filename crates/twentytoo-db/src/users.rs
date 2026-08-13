//! Users: the auth identity (`00-init` §5.1).
//!
//! Emails are normalized to lowercase on write and lookup. Hashing belongs
//! to the auth module; this layer stores the opaque `password_hash` string.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::Db;
use crate::error::DbError;

/// A user's account status.
///
/// Sign-in gating (reject `Invited` and `Disabled`) is the auth flow's
/// decision; these are the stored states it decides against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserStatus {
    /// May sign in.
    Active,
    /// Created by invite; no password set yet.
    Invited,
    /// Locked out.
    Disabled,
}

impl UserStatus {
    /// The stored column value.
    pub fn as_str(&self) -> &'static str {
        return match self {
            UserStatus::Active => "active",
            UserStatus::Invited => "invited",
            UserStatus::Disabled => "disabled",
        };
    }
}

impl TryFrom<String> for UserStatus {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        return match s.as_str() {
            "active" => Ok(UserStatus::Active),
            "invited" => Ok(UserStatus::Invited),
            "disabled" => Ok(UserStatus::Disabled),
            _ => Err(format!("unknown user status: {s}")),
        };
    }
}

/// One row of `users`.
#[derive(Clone, Debug, FromRow)]
pub struct User {
    /// Stable id.
    pub id: Uuid,
    /// Login identity, stored lowercase.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Password hash, `None` until the user sets one.
    pub password_hash: Option<String>,
    /// Account status.
    #[sqlx(try_from = "String")]
    pub status: UserStatus,
    /// Row creation time.
    pub created_at: DateTime<Utc>,
    /// Last update time.
    pub updated_at: DateTime<Utc>,
}

impl Db {
    /// Create a user; email is trimmed and lowercased. Duplicate email →
    /// [`DbError::Conflict`].
    pub async fn create_user(
        &self,
        email: &str,
        name: &str,
        password_hash: Option<&str>,
    ) -> Result<User, DbError> {
        let email = email.trim().to_lowercase();
        let row = sqlx::query_as::<_, User>(
            "INSERT INTO users (email, name, password_hash)
             VALUES ($1, $2, $3)
             RETURNING id, email, name, password_hash, status, created_at, updated_at",
        )
        .bind(&email)
        .bind(name)
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Look up a user by id.
    pub async fn get_user(&self, id: &Uuid) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, User>(
            "SELECT id, email, name, password_hash, status, created_at, updated_at
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Look up a user by email; the query is case-insensitive because all
    /// writes normalize to lowercase.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, User>(
            "SELECT id, email, name, password_hash, status, created_at, updated_at
             FROM users WHERE email = $1",
        )
        .bind(email.trim().to_lowercase())
        .fetch_optional(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Replace the password hash. [`DbError::NotFound`] when the user does
    /// not exist.
    pub async fn set_user_password(&self, id: &Uuid, password_hash: &str) -> Result<(), DbError> {
        let affected =
            sqlx::query("UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2")
                .bind(password_hash)
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound);
        }
        return Ok(());
    }

    /// Set the account status. [`DbError::NotFound`] when the user does
    /// not exist.
    pub async fn set_user_status(&self, id: &Uuid, status: UserStatus) -> Result<(), DbError> {
        let affected =
            sqlx::query("UPDATE users SET status = $1, updated_at = now() WHERE id = $2")
                .bind(status.as_str())
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound);
        }
        return Ok(());
    }
}
