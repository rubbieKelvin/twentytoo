//! The demo app: two resources on the in-memory adapter (`03` §15 — a
//! checkout with no database still boots), driven through the generated
//! CRUD views. Run: `cargo run -p twentytoo --example demo`.

use std::sync::Arc;

use twentytoo::prelude::*;

/// Demo-only policy: everything allowed. The auth/RBAC slice replaces this
/// with real role-gated policies.
struct AllowAll;

impl<E> Policy<E> for AllowAll {
    fn can_view_any(&self, _actor: &Actor) -> bool {
        return true;
    }

    fn can_create(&self, _actor: &Actor) -> bool {
        return true;
    }

    fn can_update(&self, _actor: &Actor, _record: &E) -> bool {
        return true;
    }

    fn can_delete(&self, _actor: &Actor, _record: &E) -> bool {
        return true;
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct User {
    id: String,
    name: String,
    email: String,
    role: String,
    status: String,
    /// Server-managed: forms never send it, so it needs a default.
    #[serde(default)]
    created_at: String,
}

struct UserResource {
    adapter: Arc<InMemoryAdapter<User>>,
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Store {
    id: String,
    name: String,
    city: String,
    status: String,
    revenue: f64,
    /// Server-managed: forms never send it, so it needs a default.
    #[serde(default)]
    created_at: String,
}

struct StoreResource {
    adapter: Arc<InMemoryAdapter<Store>>,
}

impl Resource for StoreResource {
    type Entity = Store;

    fn key(&self) -> &'static str {
        return "stores";
    }

    fn label(&self) -> &'static str {
        return "Stores";
    }

    fn fields(&self) -> Vec<Field<Self::Entity>> {
        return fields![
            field!("id", "Id", Text, form: true, required: true),
            field!("name", "Name", Text, list: true, detail: true, form: true, required: true, sortable: true, searchable: true),
            field!("city", "City", Text, list: true, detail: true, form: true, sortable: true),
            field!("status", "Status", Badge { options: &[("open", "Open"), ("closed", "Closed")] }, list: true, detail: true, form: true),
            field!("revenue", "Revenue", Currency, list: true, detail: true, form: true, sortable: true),
            field!("created_at", "Created", DateTime, list: true, detail: true, sortable: true),
        ];
    }

    fn list_columns(&self) -> Vec<&'static str> {
        return vec!["name", "city", "status", "revenue", "created_at"];
    }

    fn default_sort(&self) -> Vec<SortField> {
        return vec![SortField::desc("created_at")];
    }

    fn search_fields(&self) -> Vec<&'static str> {
        return vec!["name", "city"];
    }

    fn filters(&self) -> Vec<FilterSpec> {
        return vec![FilterSpec {
            field: "status",
            op: FilterOp::Eq,
            label: Some("Status"),
        }];
    }

    fn policy(&self) -> &dyn Policy<Self::Entity> {
        return &AllowAll;
    }

    fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>> {
        return self.adapter.clone();
    }
}

fn seed_users(adapter: &Arc<InMemoryAdapter<User>>) {
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

fn seed_stores(adapter: &Arc<InMemoryAdapter<Store>>) {
    // Thirty stores: enough to see the numbered pager at the default 25/page.
    for i in 1..=30 {
        let id = format!("s{i}");
        adapter
            .insert(
                id.clone(),
                Store {
                    id,
                    name: format!("Store {i:02}"),
                    city: ["Berlin", "London", "Paris", "Madrid", "Rome"][(i - 1) % 5].to_string(),
                    status: if i % 7 == 0 {
                        "closed".to_string()
                    } else {
                        "open".to_string()
                    },
                    revenue: (i * 137) as f64 * 1.5,
                    created_at: format!("2026-07-{:02}T09:00:00Z", (i % 28) + 1),
                },
            )
            .expect("seed id is unique");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let users = Arc::new(InMemoryAdapter::<User>::new());
    seed_users(&users);
    let stores = Arc::new(InMemoryAdapter::<Store>::new());
    seed_stores(&stores);

    let app = twentytoo::Twentytoo::builder()
        .resource(UserResource { adapter: users })
        .resource(StoreResource { adapter: stores })
        .default_actor(Actor {
            id: "admin".to_string(),
            email: "admin@example.com".to_string(),
            roles: vec!["admin".to_string()],
            permissions: vec!["*.*".to_string()],
            team_id: None,
        })
        .build()
        .await?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("twentytoo demo → http://127.0.0.1:3000");
    axum::serve(listener, app.into_make_service()).await?;
    return Ok(());
}
