//! Roles, permissions, and the actor loader — RBAC (`00-init` §5).
//!
//! A permission is a `resource.action` code; a role bundles permissions;
//! a user holds roles globally or within a team. [`Db::load_actor`] is the
//! centerpiece: it expands a user's grants into the [`Actor`] the request
//! pipeline sees, matching the core contract where `Actor.permissions` are
//! "expanded from roles".

use sqlx::FromRow;
use twentytoo_core::Actor;
use uuid::Uuid;

use crate::Db;
use crate::error::DbError;

/// One row of `permissions`.
#[derive(Clone, Debug, FromRow)]
pub struct Permission {
    /// Stable id.
    pub id: Uuid,
    /// The `resource.action` code, e.g. `"stores.view"`.
    pub code: String,
    /// Human explanation of what the permission allows.
    pub description: String,
}

/// One row of `roles`.
#[derive(Clone, Debug, FromRow)]
pub struct Role {
    /// Stable id.
    pub id: Uuid,
    /// Stable key, e.g. `"admin"`.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Human explanation of what the role bundles.
    pub description: String,
}

/// Whether `code` is a valid permission code: two non-empty
/// `[a-z0-9_*]`-only segments joined by `.` — exactly the shape
/// [`Actor::can`] matches (`core/actor.rs`: two segments, per-segment
/// wildcards, case-sensitive).
pub fn validate_permission_code(code: &str) -> bool {
    let Some((resource, action)) = code.split_once('.') else {
        return false;
    };
    if action.is_empty() || action.contains('.') {
        return false;
    }
    let segment_ok = |s: &str| -> bool {
        return !s.is_empty()
            && s.chars().all(|c| {
                return c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '*';
            });
    };
    return segment_ok(resource) && segment_ok(action);
}

/// One joined row of the actor-expansion query.
#[derive(FromRow)]
struct GrantRow {
    role_key: String,
    permission_code: Option<String>,
}

impl Db {
    /// Register a permission. Malformed code → [`DbError::Validation`];
    /// duplicate code → [`DbError::Conflict`].
    pub async fn create_permission(
        &self,
        code: &str,
        description: &str,
    ) -> Result<Permission, DbError> {
        if !validate_permission_code(code) {
            return Err(DbError::Validation(format!(
                "invalid permission code: {code}"
            )));
        }
        let row = sqlx::query_as::<_, Permission>(
            "INSERT INTO permissions (code, description) VALUES ($1, $2)
             RETURNING id, code, description",
        )
        .bind(code)
        .bind(description)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Register a role. Duplicate key → [`DbError::Conflict`].
    pub async fn create_role(
        &self,
        key: &str,
        name: &str,
        description: &str,
    ) -> Result<Role, DbError> {
        let row = sqlx::query_as::<_, Role>(
            "INSERT INTO roles (key, name, description) VALUES ($1, $2, $3)
             RETURNING id, key, name, description",
        )
        .bind(key)
        .bind(name)
        .bind(description)
        .fetch_one(&self.pool)
        .await?;
        return Ok(row);
    }

    /// Grant a permission to a role; already granted → no-op.
    pub async fn grant_permission(
        &self,
        role_id: &Uuid,
        permission_id: &Uuid,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO role_permissions (role_id, permission_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// Revoke a permission from a role; not granted → no-op.
    pub async fn revoke_permission(
        &self,
        role_id: &Uuid,
        permission_id: &Uuid,
    ) -> Result<(), DbError> {
        sqlx::query("DELETE FROM role_permissions WHERE role_id = $1 AND permission_id = $2")
            .bind(role_id)
            .bind(permission_id)
            .execute(&self.pool)
            .await?;
        return Ok(());
    }

    /// The permissions bundled by one role, ordered by code.
    pub async fn list_role_permissions(&self, role_id: &Uuid) -> Result<Vec<Permission>, DbError> {
        let rows = sqlx::query_as::<_, Permission>(
            "SELECT p.id, p.code, p.description
             FROM permissions p
             JOIN role_permissions rp ON rp.permission_id = p.id
             WHERE rp.role_id = $1
             ORDER BY p.code",
        )
        .bind(role_id)
        .fetch_all(&self.pool)
        .await?;
        return Ok(rows);
    }

    /// Assign a role to a user. `team_id` `None` = global grant; `Some` =
    /// the role applies only within that team. Already assigned → no-op
    /// (the `UNIQUE NULLS NOT DISTINCT` constraint keeps grants unique).
    pub async fn assign_role(
        &self,
        user_id: &Uuid,
        role_id: &Uuid,
        team_id: Option<&Uuid>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id, team_id) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(role_id)
        .bind(team_id)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// Remove a role grant; not granted → no-op.
    pub async fn revoke_role(
        &self,
        user_id: &Uuid,
        role_id: &Uuid,
        team_id: Option<&Uuid>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "DELETE FROM user_roles WHERE user_id = $1 AND role_id = $2
             AND team_id IS NOT DISTINCT FROM $3",
        )
        .bind(user_id)
        .bind(role_id)
        .bind(team_id)
        .execute(&self.pool)
        .await?;
        return Ok(());
    }

    /// The full actor for a user: identity plus roles and permissions
    /// expanded from every grant that applies — global grants always, and
    /// team-scoped grants when `team_id` matches. `None` when the user does
    /// not exist.
    ///
    /// Status gating (rejecting `invited`/`disabled`) is the auth flow's
    /// job at sign-in; this loads whatever the database holds.
    pub async fn load_actor(
        &self,
        user_id: &Uuid,
        team_id: Option<&Uuid>,
    ) -> Result<Option<Actor>, DbError> {
        let user = sqlx::query_as::<_, crate::users::User>(
            "SELECT id, email, name, password_hash, status, created_at, updated_at
             FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(user) = user else {
            return Ok(None);
        };

        let grants = sqlx::query_as::<_, GrantRow>(
            "SELECT r.key AS role_key, p.code AS permission_code
             FROM user_roles ur
             JOIN roles r ON r.id = ur.role_id
             LEFT JOIN role_permissions rp ON rp.role_id = r.id
             LEFT JOIN permissions p ON p.id = rp.permission_id
             WHERE ur.user_id = $1
               AND (ur.team_id IS NULL OR ur.team_id = $2)
             ORDER BY r.key, p.code",
        )
        .bind(user_id)
        .bind(team_id)
        .fetch_all(&self.pool)
        .await?;

        let mut roles: Vec<String> = Vec::new();
        let mut permissions: Vec<String> = Vec::new();
        for grant in grants {
            // Ordered by role key, so equality with the last push dedups.
            if !roles.last().is_some_and(|k| return k == &grant.role_key) {
                roles.push(grant.role_key);
            }
            if let Some(code) = grant.permission_code {
                permissions.push(code);
            }
        }

        return Ok(Some(Actor {
            id: user.id.to_string(),
            email: user.email,
            roles,
            permissions,
            team_id: team_id.map(|t| return t.to_string()),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_permission_codes_pass() {
        for code in [
            "stores.view",
            "*.view",
            "stores.*",
            "doctors.approve_2",
            "a.b",
        ] {
            assert!(validate_permission_code(code), "{code} should be valid");
        }
    }

    #[test]
    fn malformed_permission_codes_fail() {
        for code in [
            "",
            "stores",
            "stores.view.extra",
            "Stores.view",
            "stores.view!",
            "a..b",
            ".view",
            "stores.",
            "stores view",
        ] {
            assert!(!validate_permission_code(code), "{code} should be invalid");
        }
    }
}
