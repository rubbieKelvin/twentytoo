//! The acting principal: who is doing what.

/// The actor behind a request — the subject of policies, actions, and audit.
///
/// Permission strings are `"resource.action"`. A stored entry matches a
/// requested permission iff it has exactly two segments and each stored
/// segment equals the requested segment or is `"*"` (`*.view` matches
/// `stores.view`; `stores.*` matches `stores.view`). Entries without a `.`,
/// with more than two segments, or with a case difference never match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    /// Stable user id.
    pub id: String,
    /// Email address.
    pub email: String,
    /// Role names (e.g. `"admin"`, `"ops"`).
    pub roles: Vec<String>,
    /// Granted permissions, `"resource.action"` format.
    pub permissions: Vec<String>,
    /// Team scope, if the actor belongs to one.
    pub team_id: Option<String>,
}

impl Actor {
    /// Whether any stored permission matches `permission`.
    ///
    /// Matching is case-sensitive, two-segment, wildcard-per-segment.
    pub fn can(&self, permission: &str) -> bool {
        let Some((req_res, req_act)) = split_once(permission) else {
            return false;
        };
        self.permissions.iter().any(|entry| {
            split_once(entry).is_some_and(|(res, act)| {
                (res == "*" || res == req_res) && (act == "*" || act == req_act)
            })
        })
    }

    /// Whether the actor holds `role` exactly.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Split `"resource.action"` on the first `.`; `None` when malformed.
fn split_once(s: &str) -> Option<(&str, &str)> {
    let (res, rest) = s.split_once('.')?;
    if rest.is_empty() || rest.contains('.') {
        return None;
    }
    Some((res, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(permissions: &[&str]) -> Actor {
        Actor {
            id: "u1".into(),
            email: "u1@example.com".into(),
            roles: vec![],
            permissions: permissions.iter().map(|s| s.to_string()).collect(),
            team_id: None,
        }
    }

    #[test]
    fn exact_match() {
        let a = actor(&["stores.view", "products.edit"]);
        assert!(a.can("stores.view"));
        assert!(a.can("products.edit"));
    }

    #[test]
    fn wildcard_resource_matches() {
        let a = actor(&["*.view"]);
        assert!(a.can("stores.view"));
        assert!(a.can("orders.view"));
    }

    #[test]
    fn wildcard_action_matches() {
        let a = actor(&["stores.*"]);
        assert!(a.can("stores.view"));
        assert!(a.can("stores.delete"));
    }

    #[test]
    fn mismatched_resource_denied() {
        let a = actor(&["stores.view"]);
        assert!(!a.can("products.view"));
    }

    #[test]
    fn mismatched_action_denied() {
        let a = actor(&["stores.view"]);
        assert!(!a.can("stores.delete"));
    }

    #[test]
    fn malformed_entries_never_match() {
        let a = actor(&["nodot", "a.b.c", "trailing.", ".leading"]);
        assert!(!a.can("nodot"));
        assert!(!a.can("a.b"));
        assert!(!a.can("trailing"));
        assert!(!a.can("leading"));
    }

    #[test]
    fn case_sensitive() {
        let a = actor(&["Stores.View"]);
        assert!(!a.can("stores.view"));
    }

    #[test]
    fn no_match() {
        let a = actor(&["stores.view"]);
        assert!(!a.can("audit.view"));
        assert!(!a.can("stores"));
    }

    #[test]
    fn empty_permissions_deny_all() {
        let a = actor(&[]);
        assert!(!a.can("stores.view"));
    }
}
