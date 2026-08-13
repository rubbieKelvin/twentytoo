//! The composition root / service container: builder → axum router (`01` §4.1).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::middleware;
use axum::routing::get;
use twentytoo_core::{Actor, DataError, Resource};

use crate::infrastructure::flags::FlagService;
use crate::infrastructure::templates::TemplateEngine;
use crate::presentation::handlers::{ResourceState, home_handler, not_found, resource_routes};
use crate::presentation::middleware::actor_layer;
use crate::presentation::registry::{DynResourceMeta, ResourceMeta, ResourceRegistry};
use crate::presentation::state::AppState;
use crate::shared::errors::BuildError;

/// The built framework instance; hand [`Twentytoo::into_make_service`] to
/// axum, or [`Twentytoo::into_router`] to nest inside a larger app.
pub struct Twentytoo {
    router: Router<AppState>,
    state: AppState,
}

impl Twentytoo {
    /// Start building.
    pub fn builder() -> TwentytooBuilder {
        return TwentytooBuilder::default();
    }

    /// The axum router with every resource, route, and middleware wired.
    ///
    /// The router's state is [`AppState`]; call `.with_state(…)` with the
    /// same values to bake it in, or use [`Twentytoo::into_make_service`].
    pub fn into_router(self) -> Router<AppState> {
        return self.router;
    }

    /// The router with its state baked in — ready for `axum::serve`.
    pub fn into_make_service(self) -> Router<()> {
        return self.router.with_state(self.state);
    }
}

/// One registered resource, erased for heterogeneous storage.
///
/// The router is monomorphized per resource (`resource_routes::<R>`); the
/// meta is what the non-generic handlers (nav, home) see.
#[async_trait]
pub trait ErasedResource: Send + Sync {
    /// Resource key.
    fn key(&self) -> &'static str;
    /// Boot validation (`03` §11.3): every declared identifier must exist
    /// in the source.
    async fn validate(&self) -> Result<(), DataError>;
    /// The erased meta (cheap clone of the resource handle).
    fn meta(&self) -> Box<dyn DynResourceMeta>;
    /// Consume into the per-resource sub-router.
    fn into_router(self: Box<Self>, app: Arc<AppState>) -> Router<()>;
}

#[async_trait]
#[allow(clippy::implicit_return)]
impl<R: Resource> ErasedResource for ResourceMeta<R> {
    fn key(&self) -> &'static str {
        return self.resource.key();
    }

    async fn validate(&self) -> Result<(), DataError> {
        let resource = &*self.resource;
        let mut identifiers: Vec<String> = resource
            .fields()
            .iter()
            .map(|f| return f.name.to_string())
            .collect();
        identifiers.extend(resource.list_columns().iter().map(|s| return s.to_string()));
        identifiers.extend(
            resource
                .search_fields()
                .iter()
                .map(|s| return s.to_string()),
        );
        let default_sort = resource.default_sort();
        identifiers.extend(default_sort.iter().map(|s| return s.field.clone()));
        identifiers.extend(
            resource
                .filters()
                .iter()
                .map(|f| return f.field.to_string()),
        );
        identifiers.sort_unstable();
        identifiers.dedup();
        let refs: Vec<&str> = identifiers.iter().map(|s| return s.as_str()).collect();
        return resource.adapter().validate(&refs).await;
    }

    fn meta(&self) -> Box<dyn DynResourceMeta> {
        return Box::new(ResourceMeta {
            resource: self.resource.clone(),
        });
    }

    fn into_router(self: Box<Self>, app: Arc<AppState>) -> Router<()> {
        let resource = self.resource.clone();
        let state = ResourceState {
            app,
            resource: resource.clone(),
        };
        return resource_routes::<R>().with_state(state);
    }
}

/// The declarative surface: resources, templates, identity (`01` §7.1).
#[derive(Default)]
pub struct TwentytooBuilder {
    resources: Vec<Box<dyn ErasedResource>>,
    template_dir: Option<PathBuf>,
    default_actor: Option<Actor>,
}

impl TwentytooBuilder {
    /// Register a resource.
    pub fn resource<R: Resource>(mut self, resource: R) -> Self {
        self.resources.push(Box::new(ResourceMeta {
            resource: Arc::new(resource),
        }));
        return self;
    }

    /// Directory of user templates overriding the built-ins (`05` §5.3).
    pub fn with_template_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.template_dir = Some(dir.into());
        return self;
    }

    /// The actor assumed for requests without a session. Until auth lands
    /// (`01` Step 5) this is the only identity.
    pub fn default_actor(mut self, actor: Actor) -> Self {
        self.default_actor = Some(actor);
        return self;
    }

    /// Validate, build, and return the framework instance.
    ///
    /// Fails at boot — not at first click — when a declared identifier is
    /// missing from its source (`03` §11.3) or a template name does not
    /// resolve (`05` §12).
    pub async fn build(self) -> Result<Twentytoo, BuildError> {
        // Fail at boot: every resource's identifiers must validate.
        for resource in &self.resources {
            resource.validate().await?;
        }

        let templates = Arc::new(TemplateEngine::new(self.template_dir.as_deref())?);
        let default_actor = self.default_actor.unwrap_or(Actor {
            id: "anonymous".to_string(),
            email: "anonymous@localhost".to_string(),
            roles: Vec::new(),
            permissions: Vec::new(),
            team_id: None,
        });

        let metas: Vec<Box<dyn DynResourceMeta>> =
            self.resources.iter().map(|r| return r.meta()).collect();
        let app = Arc::new(AppState {
            registry: Arc::new(ResourceRegistry::new(metas)),
            templates,
            flags: Arc::new(FlagService::new()),
            default_actor,
        });

        let mut router: Router<AppState> = Router::new()
            .route("/", get(home_handler))
            .fallback(not_found);
        for resource in self.resources {
            let key = resource.key().to_string();
            // Each resource router carries its own `ResourceState<R>` and is
            // state-baked (`Router<()>`) before nesting, so the outer
            // `Router<AppState>` stays uniform.
            router = router.nest_service(&format!("/{key}"), resource.into_router(app.clone()));
        }
        // The layer's state must be `AppState` itself — `State<AppState>`
        // extracts via `FromRef` and `Arc<AppState>` has no such impl.
        router = router.layer(middleware::from_fn_with_state((*app).clone(), actor_layer));

        return Ok(Twentytoo {
            router,
            state: (*app).clone(),
        });
    }
}
