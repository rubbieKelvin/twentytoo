//! Shared application state.

use std::sync::Arc;

use twentytoo_core::Actor;

use crate::application::auth::AuthService;
use crate::infrastructure::flags::FlagService;
use crate::infrastructure::templates::TemplateEngine;
use crate::presentation::registry::{NavItem, ResourceRegistry};

/// Everything handlers share: resources, templates, flags, identity.
#[derive(Clone)]
pub struct AppState {
    /// One erased meta per registered resource (nav, home cards).
    pub registry: Arc<ResourceRegistry>,
    /// The built template environment.
    pub templates: Arc<TemplateEngine>,
    /// Feature-flag state.
    pub flags: Arc<FlagService>,
    /// The actor assumed for requests without a session. Only used when
    /// auth is not configured; with auth, the middleware resolves the
    /// real actor from the session cookie.
    pub default_actor: Actor,
    /// The login/user-management service. `None` = auth disabled: the
    /// middleware injects `default_actor` and no auth routes are mounted.
    pub auth: Option<Arc<AuthService>>,
}

impl AppState {
    /// The nav entries for `actor`: the registry's resources, plus the
    /// Users area when auth is enabled and the actor may view it.
    pub fn nav_for(&self, actor: &Actor) -> Vec<NavItem> {
        let mut nav = self.registry.nav();
        if self.auth.is_some() && actor.can("users.view") {
            nav.push(NavItem {
                key: "users",
                label: "Users",
                icon: "users",
            });
        }
        return nav;
    }
}
