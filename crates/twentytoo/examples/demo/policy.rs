//! Demo-only policy: everything allowed. The auth/RBAC slice replaces this
//! with real role-gated policies.

use twentytoo::prelude::*;

/// A `Policy` that permits every operation, shared by both demo resources.
pub struct AllowAll;

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
