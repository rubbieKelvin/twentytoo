//! The composition root / service container: builder → axum router (`00` §7.2).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use twentytoo_core::{Actor, DataError, Resource};
use twentytoo_db::Db;

use crate::application::auth::{AuthConfig, AuthService, CodeSender, ConsoleCodeSender};
use crate::infrastructure::flags::FlagService;
use crate::infrastructure::static_files::StaticFiles;
use crate::infrastructure::templates::{ICON_NAMES, TemplateEngine};
use crate::presentation::handlers::{
    ResourceState, auth, home_handler, not_found, resource_routes, static_file_handler, users,
};
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
    /// Boot validation (`00` §7.2): every declared identifier must exist
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

/// The declarative surface: resources, templates, identity (`00` §7.2).
#[derive(Default)]
pub struct TwentytooBuilder {
    resources: Vec<Box<dyn ErasedResource>>,
    template_dir: Option<PathBuf>,
    default_actor: Option<Actor>,
    /// PostgreSQL URL for the framework-owned tables (auth, sessions,
    /// users). Connected during [`TwentytooBuilder::build`]; required
    /// when [`TwentytooBuilder::auth`] is configured.
    db_url: Option<String>,
    /// Apply the embedded schema migrations at build.
    migrate: bool,
    /// Auth configuration; `None` = auth disabled (today's behavior).
    auth: Option<AuthConfig>,
    /// The login-code delivery channel; defaults to console logging.
    code_sender: Option<Box<dyn CodeSender>>,
}

impl TwentytooBuilder {
    /// Register a resource.
    pub fn resource<R: Resource>(mut self, resource: R) -> Self {
        self.resources.push(Box::new(ResourceMeta {
            resource: Arc::new(resource),
        }));
        return self;
    }

    /// Directory of user templates overriding the built-ins (`00` §8.5).
    pub fn with_template_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.template_dir = Some(dir.into());
        return self;
    }

    /// The actor assumed for requests without a session. Until auth lands
    /// (`00` §6.3) this is the only identity.
    pub fn default_actor(mut self, actor: Actor) -> Self {
        self.default_actor = Some(actor);
        return self;
    }

    /// PostgreSQL URL for the framework-owned tables (auth, sessions,
    /// users). Connected during [`TwentytooBuilder::build`]; required
    /// when [`TwentytooBuilder::auth`] is configured.
    pub fn db(mut self, url: impl Into<String>) -> Self {
        self.db_url = Some(url.into());
        return self;
    }

    /// Apply the embedded schema migrations to the configured database
    /// during [`TwentytooBuilder::build`]. Requires [`TwentytooBuilder::db`].
    pub fn migrate(mut self) -> Self {
        self.migrate = true;
        return self;
    }

    /// Enable auth: the login flow, sessions, and the permission-gated
    /// `/users` area. The middleware switches from `default_actor` to
    /// session-based actor extraction.
    pub fn auth(mut self, config: AuthConfig) -> Self {
        self.auth = Some(config);
        return self;
    }

    /// Replace the console default with a real delivery channel for login
    /// codes.
    pub fn code_sender(mut self, sender: Box<dyn CodeSender>) -> Self {
        self.code_sender = Some(sender);
        return self;
    }

    /// Validate, build, and return the framework instance.
    ///
    /// Fails at boot — not at first click — when a declared identifier is
    /// missing from its source (`00` §7.2) or a template name does not
    /// resolve (`00` §8.5).
    pub async fn build(self) -> Result<Twentytoo, BuildError> {
        // Fail at boot: every resource's identifiers must validate.
        for resource in &self.resources {
            resource.validate().await?;
        }

        // Auth preconditions: the framework-owned tables must be reachable,
        // and the "users" key belongs to the built-in user area.
        if self.auth.is_some() && self.db_url.is_none() {
            return Err(BuildError::Config(
                "auth requires a database: call .db(...)".to_string(),
            ));
        }
        if self.migrate && self.db_url.is_none() {
            return Err(BuildError::Config(
                "migrate requires a database: call .db(...)".to_string(),
            ));
        }
        if self.auth.is_some() && self.resources.iter().any(|r| return r.key() == "users") {
            return Err(BuildError::Config(
                "auth owns the /users routes; a workspace resource named \"users\" conflicts"
                    .to_string(),
            ));
        }

        // The framework-owned tables: connect once, migrate on demand.
        // Fail at boot — not at first login — when they are unreachable.
        let db = match &self.db_url {
            Some(url) => {
                let db = Db::connect(url)
                    .await
                    .map_err(|e| return BuildError::Db(format!("connect to {url}: {e}")))?;
                if self.migrate {
                    db.migrate()
                        .await
                        .map_err(|e| return BuildError::Db(format!("migrate {url}: {e}")))?;
                }
                Some(db)
            }
            None => None,
        };

        let templates = Arc::new(TemplateEngine::new(self.template_dir.as_deref())?);
        // Boot check (`00` §8.6): every asset the built-in templates
        // reference must be embedded in the binary.
        for name in StaticFiles::BUILTIN_ASSETS {
            StaticFiles::get(name).ok_or_else(|| {
                return BuildError::Config(format!(
                    "built-in static asset missing from the binary: {name}"
                ));
            })?;
        }
        let default_actor = self.default_actor.unwrap_or(Actor {
            id: "anonymous".to_string(),
            email: "anonymous@localhost".to_string(),
            roles: Vec::new(),
            permissions: Vec::new(),
            team_id: None,
        });

        // Auth: bootstrap the admin role/permissions/user once, then share
        // the service with the state and the middleware.
        let auth = match self.auth {
            Some(config) => {
                let db = db.clone().expect("checked: auth needs a database");
                let sender = self.code_sender.unwrap_or(Box::new(ConsoleCodeSender));
                let service = Arc::new(AuthService::new(db, config, sender));
                service
                    .bootstrap()
                    .await
                    .map_err(|e| return BuildError::Data(e.into()))?;
                Some(service)
            }
            None => None,
        };

        let metas: Vec<Box<dyn DynResourceMeta>> =
            self.resources.iter().map(|r| return r.meta()).collect();
        // Boot check (`01-ui-kit` §11.3): every resource icon must resolve
        // in the closed icon set — an unknown name fails here, not at
        // first render.
        for meta in &metas {
            if !ICON_NAMES.contains(&meta.icon()) {
                return Err(BuildError::Config(format!(
                    "resource \"{}\" declares unknown icon \"{}\" — add it to the icon set",
                    meta.key(),
                    meta.icon()
                )));
            }
        }
        let app = Arc::new(AppState {
            registry: Arc::new(ResourceRegistry::new(metas)),
            templates,
            flags: Arc::new(FlagService::new()),
            default_actor,
            auth,
        });

        let mut router: Router<AppState> = Router::new()
            .route("/", get(home_handler))
            .route("/static/{*path}", get(static_file_handler))
            .fallback(not_found);
        if app.auth.is_some() {
            router = router
                .route("/login", get(auth::login_screen))
                .route("/login/email", post(auth::login_email_handler))
                .route(
                    "/login/code",
                    get(auth::code_screen).post(auth::login_code_handler),
                )
                .route(
                    "/login/password",
                    get(auth::password_screen).post(auth::login_password_handler),
                )
                .route("/logout", post(auth::logout_handler))
                .route("/users", get(users::list_handler))
                .route(
                    "/users/new",
                    get(users::create_form).post(users::create_handler),
                )
                .route(
                    "/users/{id}",
                    get(users::edit_form).post(users::update_handler),
                );
        }
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
