//! Per-resource authorization: what an actor may do with records.
//!
//! Every method defaults to **deny** — rendering without checking
//! permissions is a bug, and an unconfigured policy must be safe.

use crate::actor::Actor;

/// Row- and operation-level access control for one resource.
///
/// Record methods receive the entity; the engine calls them wherever it has
/// the record in hand (detail views, actions, mutation replay). Adapters
/// never see policies — the engine merges the policy scope into the query
/// filter instead.
pub trait Policy<E>: Send + Sync {
    /// May the actor see *any* record of this resource (list views)?
    fn can_view_any(&self, _actor: &Actor) -> bool {
        false
    }

    /// May the actor see this specific record?
    fn can_view(&self, actor: &Actor, _record: &E) -> bool {
        self.can_view_any(actor)
    }

    /// May the actor create records?
    fn can_create(&self, _actor: &Actor) -> bool {
        false
    }

    /// May the actor update this record?
    fn can_update(&self, _actor: &Actor, _record: &E) -> bool {
        false
    }

    /// May the actor delete this record?
    fn can_delete(&self, _actor: &Actor, _record: &E) -> bool {
        false
    }
}

/// The baseline policy: nothing is allowed.
pub struct DenyAll;

impl<E> Policy<E> for DenyAll {
    fn can_view_any(&self, _actor: &Actor) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> Actor {
        Actor {
            id: "u1".into(),
            email: "u1@example.com".into(),
            roles: vec![],
            permissions: vec![],
            team_id: None,
        }
    }

    struct Row;

    struct ViewAny;

    impl<E> Policy<E> for ViewAny {
        fn can_view_any(&self, _actor: &Actor) -> bool {
            true
        }
    }

    #[test]
    fn deny_all_denies_everything() {
        let p: &dyn Policy<Row> = &DenyAll;
        let a = actor();
        assert!(!p.can_view_any(&a));
        assert!(!p.can_view(&a, &Row));
        assert!(!p.can_create(&a));
        assert!(!p.can_update(&a, &Row));
        assert!(!p.can_delete(&a, &Row));
    }

    #[test]
    fn view_any_implies_record_view() {
        let p: &dyn Policy<Row> = &ViewAny;
        let a = actor();
        assert!(p.can_view_any(&a));
        assert!(p.can_view(&a, &Row));
        assert!(!p.can_create(&a));
        assert!(!p.can_update(&a, &Row));
        assert!(!p.can_delete(&a, &Row));
    }
}
