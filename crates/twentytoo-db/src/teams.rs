//! Teams: the grouping/org boundary (`00-init` §5.1).
//!
//! Membership is a pure many-to-many; team-scoped *roles* are granted in
//! `user_roles` (see [`crate::access`]) and team-scoped *record access*
//! is a policy concern on top of `Actor.team_id`.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::Db;
use crate::error::DbError;

/// One row of `teams`.
#[derive(Clone, Debug, FromRow)]
pub struct Team {
    /// Stable id.
    pub id: Uuid,
    /// Display name.
    pub name: String,
    /// Unique URL- and reference-safe key.
    pub slug: String,
    /// Row creation time.
    pub created_at: DateTime<Utc>,
}

impl Db {
    /// Create a team. Duplicate slug → [`DbError::Conflict`].
    pub async fn create_team(&self, name: &str, slug: &str) -> Result<Team, DbError> {
        let row = sqlx::query_as::<_, Team>(
            "INSERT INTO teams (name, slug) VALUES ($1, $2)
             RETURNING id, name, slug, created_at",
        )
        .bind(name)
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Look up a team by id.
    pub async fn get_team(&self, id: &Uuid) -> Result<Option<Team>, DbError> {
        let row =
            sqlx::query_as::<_, Team>("SELECT id, name, slug, created_at FROM teams WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        return Ok(row);
    }

    /// Add a member; already a member → no-op.
    pub async fn add_member(&self, team_id: &Uuid, user_id: &Uuid) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(team_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// Remove a member; not a member → no-op.
    pub async fn remove_member(&self, team_id: &Uuid, user_id: &Uuid) -> Result<(), DbError> {
        sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        return Ok(());
    }

    /// The teams `user_id` belongs to, ordered by name.
    pub async fn list_teams_for_user(&self, user_id: &Uuid) -> Result<Vec<Team>, DbError> {
        let rows = sqlx::query_as::<_, Team>(
            "SELECT t.id, t.name, t.slug, t.created_at
             FROM teams t
             JOIN team_members m ON m.team_id = t.id
             WHERE m.user_id = $1
             ORDER BY t.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        return Ok(rows);
    }
}
