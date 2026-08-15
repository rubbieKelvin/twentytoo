//! The `stores` resource: its entity, `Resource` impl, and seed data.

use std::sync::Arc;

use twentytoo::prelude::*;

use crate::policy::AllowAll;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Store {
    id: String,
    name: String,
    city: String,
    status: String,
    /// Optional in the form: a blank payload omits it, so the entity needs
    /// a default (00 §7.3's server-managed-field rule).
    #[serde(default)]
    revenue: f64,
    /// Server-managed: forms never send it, so it needs a default.
    #[serde(default)]
    created_at: String,
}

pub struct StoreResource {
    pub adapter: Arc<InMemoryAdapter<Store>>,
}

impl Resource for StoreResource {
    type Entity = Store;

    fn key(&self) -> &'static str {
        return "stores";
    }

    fn label(&self) -> &'static str {
        return "Stores";
    }
    fn icon(&self) -> &'static str {
        return "file";
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

pub fn seed_stores(adapter: &Arc<InMemoryAdapter<Store>>) {
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
