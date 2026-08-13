//! The resource: a browsable, searchable, actionable surface over one entity
//! and one data source.

use std::sync::Arc;

use serde::{Serialize, de::DeserializeOwned};

use crate::action::Action;
use crate::adapter::DataAdapter;
use crate::field::Field;
use crate::policy::Policy;
use crate::query::SortField;

/// A tab linking this resource to a related one.
///
/// Shape derived from `00`'s `has_many(Customer, via: store_id)`: the tab
/// key and label on this resource, the related resource's key, and the
/// back-reference field on the related entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// Tab key (e.g. `"customers"`).
    pub key: String,
    /// Tab label (e.g. `"Customers"`).
    pub label: String,
    /// Related resource's key (e.g. `"customers"`).
    pub resource_key: String,
    /// Back-reference field on the related entity (e.g. `"store_id"`).
    pub foreign_key: String,
}

/// One resource of the framework: entity, fields, actions, policy, adapter.
///
/// `filters()` and `metrics()` are deliberately absent this slice — they
/// arrive as defaulted methods in the list-view / metrics slices.
pub trait Resource: Send + Sync + 'static {
    /// The typed entity this resource reads and writes.
    type Entity: Serialize + DeserializeOwned + Send + Sync + Clone + 'static;

    /// Stable key, used in URLs and permission strings (e.g. `"stores"`).
    fn key(&self) -> &'static str;

    /// Human label (e.g. `"Stores"`).
    fn label(&self) -> &'static str;

    /// Icon name for the nav (default: `"cube"`).
    fn icon(&self) -> &'static str {
        return "cube";
    }

    /// All fields of the entity, in definition order.
    fn fields(&self) -> Vec<Field<Self::Entity>>;

    /// Columns rendered in list views (names from `fields()`).
    fn list_columns(&self) -> Vec<&'static str>;

    /// Default list sort.
    fn default_sort(&self) -> Vec<SortField> {
        return vec![SortField::desc("created_at")];
    }

    /// Fields searched by the search box (names from `fields()`).
    fn search_fields(&self) -> Vec<&'static str> {
        return Vec::new();
    }

    /// Tabs to related resources.
    fn relationships(&self) -> Vec<Relationship> {
        return Vec::new();
    }

    /// Custom actions available on this resource.
    fn actions(&self) -> Vec<Box<dyn Action<Self::Entity>>> {
        return Vec::new();
    }

    /// Row-level access policy.
    fn policy(&self) -> &dyn Policy<Self::Entity>;

    /// Feature flag gating this resource.
    fn flag(&self) -> Option<&'static str> {
        return None;
    }

    /// The data source, built in `Module::init` where pools and clients live.
    fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::DenyAll;
    use crate::{field, fields};

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct Store {
        id: String,
        name: String,
        status: String,
    }

    struct StoreResource {
        adapter: Arc<dyn DataAdapter<Store>>,
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
                field!("id", "Id", Text, list: true),
                field!("name", "Name", Text, list: true, detail: true, form: true, required: true),
                field!("status", "Status", Badge { options: &[("active", "Active"), ("closed", "Closed")] }, list: true),
            ];
        }

        fn list_columns(&self) -> Vec<&'static str> {
            return vec!["id", "name", "status"];
        }

        fn default_sort(&self) -> Vec<SortField> {
            return vec![SortField::desc("created_at")];
        }

        fn policy(&self) -> &dyn Policy<Self::Entity> {
            return &DenyAll;
        }

        fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>> {
            return self.adapter.clone();
        }
    }

    #[test]
    fn macro_and_trait_surface_compose() {
        let r = StoreResource {
            adapter: Arc::new(crate::in_memory::InMemoryAdapter::<Store>::new()),
        };
        assert_eq!(r.key(), "stores");
        assert_eq!(r.label(), "Stores");

        let f = r.fields();
        assert_eq!(f.len(), 3);
        assert_eq!(f[0].name, "id");
        assert!(f[0].show_in_list);
        assert!(f[1].required);
        assert_eq!(
            f[2].kind,
            crate::field::FieldKind::Badge {
                options: vec![("active", "Active"), ("closed", "Closed")]
            }
        );

        assert_eq!(r.list_columns(), vec!["id", "name", "status"]);
        assert_eq!(r.default_sort(), vec![SortField::desc("created_at")]);
        assert!(r.search_fields().is_empty());
        assert!(r.relationships().is_empty());
        assert!(r.actions().is_empty());
        assert!(!r.policy().can_view_any(&crate::actor::Actor {
            id: "u".into(),
            email: "u@example.com".into(),
            roles: vec![],
            permissions: vec![],
            team_id: None,
        }));
    }
}
