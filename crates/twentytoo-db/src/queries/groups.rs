//! Group queries: the typed access layer over `groups` + `group_members`.
//!
//! Membership is a pure many-to-many — a user belongs to any number of
//! groups. Roles are granted to groups in [`crate::queries::access`] (`group_roles`),
//! and every member inherits them. The row shape lives in
//! [`crate::entities::group`].

use uuid::Uuid;

use crate::Db;
use crate::entities::{Group, User};
use crate::error::DbError;

impl Db {
    /// Create a group. Duplicate slug → [`DbError::Conflict`].
    pub async fn create_group(&self, name: &str, slug: &str) -> Result<Group, DbError> {
        let row = sqlx::query_as::<_, Group>(
            "INSERT INTO groups (name, slug) VALUES ($1, $2)
             RETURNING id, name, slug, created_at",
        )
        .bind(name)
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Look up a group by id.
    pub async fn get_group(&self, id: &Uuid) -> Result<Option<Group>, DbError> {
        let row = sqlx::query_as::<_, Group>(
            "SELECT id, name, slug, created_at FROM groups WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Add a member; already a member → no-op.
    pub async fn add_member(&self, group_id: &Uuid, user_id: &Uuid) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(group_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// Remove a member; not a member → no-op.
    pub async fn remove_member(&self, group_id: &Uuid, user_id: &Uuid) -> Result<(), DbError> {
        sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(group_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        return Ok(());
    }

    /// The groups `user_id` belongs to, ordered by name.
    pub async fn list_groups_for_user(&self, user_id: &Uuid) -> Result<Vec<Group>, DbError> {
        let rows = sqlx::query_as::<_, Group>(
            "SELECT g.id, g.name, g.slug, g.created_at
             FROM groups g
             JOIN group_members m ON m.group_id = g.id
             WHERE m.user_id = $1
             ORDER BY g.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        return Ok(rows);
    }

    /// The members of `group_id`, ordered by name.
    pub async fn list_group_members(&self, group_id: &Uuid) -> Result<Vec<User>, DbError> {
        let rows = sqlx::query_as::<_, User>(
            "SELECT u.id, u.email, u.name, u.password_hash, u.status, u.created_at, u.updated_at
             FROM users u
             JOIN group_members m ON m.user_id = u.id
             WHERE m.group_id = $1
             ORDER BY u.name",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;
        return Ok(rows);
    }
}
