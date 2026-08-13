//! The `users` resource: its entity, `Resource` impl, and seed data.

use std::sync::Arc;

use twentytoo::prelude::*;

use crate::policy::AllowAll;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct User {
    id: String,
    name: String,
    email: String,
    role: String,
    status: String,
    /// Server-managed: forms never send it, so it needs a default.
    #[serde(default)]
    created_at: String,
}

pub struct UserResource {
    pub adapter: Arc<InMemoryAdapter<User>>,
}

impl Resource for UserResource {
    type Entity = User;

    fn key(&self) -> &'static str {
        return "users";
    }

    fn label(&self) -> &'static str {
        return "Users";
    }

    fn fields(&self) -> Vec<Field<Self::Entity>> {
        return fields![
            field!("id", "Id", Text, form: true, required: true),
            field!("name", "Name", Text, list: true, detail: true, form: true, required: true, sortable: true, searchable: true),
            field!("email", "Email", Email, list: true, detail: true, form: true, required: true, sortable: true, searchable: true),
            field!("role", "Role", Select { options: &[("admin", "Admin"), ("ops", "Ops"), ("viewer", "Viewer")] }, list: true, detail: true, form: true, sortable: true),
            field!("status", "Status", Badge { options: &[("active", "Active"), ("invited", "Invited"), ("disabled", "Disabled")] }, list: true, detail: true, form: true),
            field!("created_at", "Created", DateTime, list: true, detail: true, sortable: true),
        ];
    }

    fn list_columns(&self) -> Vec<&'static str> {
        return vec!["name", "email", "role", "status", "created_at"];
    }

    fn default_sort(&self) -> Vec<SortField> {
        return vec![SortField::asc("name")];
    }

    fn search_fields(&self) -> Vec<&'static str> {
        return vec!["name", "email"];
    }

    fn filters(&self) -> Vec<FilterSpec> {
        return vec![
            FilterSpec {
                field: "status",
                op: FilterOp::Eq,
                label: Some("Status"),
            },
            FilterSpec {
                field: "role",
                op: FilterOp::Eq,
                label: Some("Role"),
            },
        ];
    }

    fn policy(&self) -> &dyn Policy<Self::Entity> {
        return &AllowAll;
    }

    fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>> {
        return self.adapter.clone();
    }
}

pub fn seed_users(adapter: &Arc<InMemoryAdapter<User>>) {
    for (id, name, email, role, status) in [
        ("u1", "Ada Lovelace", "ada@example.com", "admin", "active"),
        ("u2", "Grace Hopper", "grace@example.com", "ops", "active"),
        (
            "u3",
            "Linus Torvalds",
            "linus@example.com",
            "ops",
            "invited",
        ),
        (
            "u4",
            "Margaret Hamilton",
            "margaret@example.com",
            "viewer",
            "disabled",
        ),
        ("u5", "Alan Turing", "alan@example.com", "viewer", "active"),
    ] {
        adapter
            .insert(
                id.to_string(),
                User {
                    id: id.to_string(),
                    name: name.to_string(),
                    email: email.to_string(),
                    role: role.to_string(),
                    status: status.to_string(),
                    created_at: "2026-08-0".to_string() + &id[1..] + "T10:00:00Z",
                },
            )
            .expect("seed id is unique");
    }
}
