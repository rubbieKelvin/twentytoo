//! Shared application state.

use std::sync::Arc;

use twentytoo_core::Actor;

use crate::infrastructure::flags::FlagService;
use crate::infrastructure::templates::TemplateEngine;
use crate::presentation::registry::ResourceRegistry;

/// Everything handlers share: resources, templates, flags, identity.
#[derive(Clone)]
pub struct AppState {
    /// One erased meta per registered resource (nav, home cards).
    pub registry: Arc<ResourceRegistry>,
    /// The built template environment.
    pub templates: Arc<TemplateEngine>,
    /// Feature-flag state.
    pub flags: Arc<FlagService>,
    /// The actor assumed for requests without a session. Auth and sessions
    /// (`01` Step 5) replace this with real extraction; until then the
    /// middleware injects it per request.
    pub default_actor: Actor,
}
