//! User queries: the typed access layer over the `users` table.
//!
//! Emails are normalized to lowercase on write and lookup. Hashing belongs
//! to the auth module; this layer stores the opaque `password_hash` string.
//! The row shape lives in [`crate::entities::user`].

use uuid::Uuid;

use crate::Db;
use crate::entities::{User, UserStatus};
use crate::error::DbError;

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

    /// All users, ordered by display name.
    pub async fn list_users(&self) -> Result<Vec<User>, DbError> {
        let rows = sqlx::query_as::<_, User>(
            "SELECT id, email, name, password_hash, status, created_at, updated_at
             FROM users ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        return Ok(rows);
    }

    /// Replace the display name. [`DbError::NotFound`] when the user does
    /// not exist.
    pub async fn update_user_name(&self, id: &Uuid, name: &str) -> Result<(), DbError> {
        let affected = sqlx::query("UPDATE users SET name = $1, updated_at = now() WHERE id = $2")
            .bind(name)
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
