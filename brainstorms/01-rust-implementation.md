# 01-rust-implementation.md — Twentytoo in Rust

**Status:** Pre-implementation design
**Depends on:** [00-init.md](./00-init.md) (core spec)
**Date:** 2026-08-11

---

## 0. Why Rust for the reference implementation

The core spec is stack-agnostic, but the reference implementation has to pick one. Rust is the right first target for this project specifically because:

- **Compile-time safety pays off for a framework.** Twentytoo's value proposition is that misconfiguration should be caught before deploy — a missing field, a broken policy, a typo in a permission string. Rust's type system catches these at build time. A dynamic-language implementation would catch them at runtime (or worse, in production).
- **Performance is a feature for internal tools.** Not because dashboards are CPU-bound, but because latency kills operator flow. Rust's zero-cost abstractions mean policy checks, flag resolution, and template rendering never become a bottleneck even as the number of resources and rules grows.
- **The trait system maps naturally to the spec.** Resource, Policy, DataAdapter, Action — each core abstraction is a Rust trait, and the compiler enforces that implementors satisfy the contract.
- **Tower's middleware model aligns with the stack.** Axum + Tower already compose auth, logging, rate-limiting, and session management as layers — exactly the cross-cutting concerns Twentytoo needs to own.
- **No garbage collection pauses.** Internal dashboards with streaming metrics, long-lived WebSocket connections, and background exports benefit from predictable latency.

The tradeoff: iteration speed. A Python or Ruby implementation would let users hack faster, and the DSL would be less verbose. Mitigation: the framework is consumed as a library, not edited — users write resource definitions in Rust, but those definitions are thin structs and trait impls, not framework internals.

---

## 1. Crate architecture

### 1.1 Workspace layout

```
twentytoo/
  Cargo.toml              # workspace root
  crates/
    twentytoo-core/       # traits, types, policy engine — no IO deps
    twentytoo/            # framework facade — re-exports, axum integration, templates
    twentytoo-users/      # built-in user management module
    twentytoo-flags/      # feature flagging module
  examples/
    demo/                 # minimal working dashboard ("stores" + "users")
```

### 1.2 Crate responsibilities

| Crate              | What it owns                                                          | Key deps                        |
| ------------------ | --------------------------------------------------------------------- | ------------------------------- |
| `twentytoo-core`   | Traits: `Resource`, `DataAdapter`, `Policy`, `Action`, `Metric`. Types: `Field`, `Filter`, `Sort`, `PaginatedResult`, `Actor`, `AuditEntry`. Pure logic — no IO, no HTTP, no templates. | `serde`, `async-trait`, `chrono` |
| `twentytoo`        | Axum router construction from `Resource` impls. Generic CRUD handlers. Template rendering (Tera). Built-in middleware (session, RBAC guard, audit). Re-exports everything. | `axum`, `tera`, `tower`, `tower-http`, `twentytoo-core` |
| `twentytoo-users`  | `User` resource, invite/session/password flows, `AuthProvider` trait, impersonation middleware. | `twentytoo-core`, `argon2`, `rand` |
| `twentytoo-flags`  | `Flag` resource, targeting strategies, per-request resolution, `flag: "..."` integration. | `twentytoo-core`                |

### 1.3 Why not more crates?

The temptation is to split `twentytoo` into `twentytoo-engine`, `twentytoo-templates`, `twentytoo-rbac`, `twentytoo-audit`, and so on. For MVP, resist this. The cost of crate-boundary coordination (versioning, re-exports, circular dependency avoidance) outweighs the benefit until the API surfaces stabilize. A single `twentytoo` crate with internal `mod` privacy is the pragmatic starting point. Split only when:

- A module has a distinct versioning lifecycle (e.g., `twentytoo-users` ships auth flows that change less often than core).
- A module has heavy dependencies that consumers shouldn't pay for by default (e.g., `twentytoo-flags` depends on nothing special, so it could stay fused — but the conceptual boundary is clean enough to split early).

The `examples/demo/` crate is load-bearing: it's the primary integration test and the thing that proves the framework works end-to-end.

---

## 2. Core trait design

### 2.1 The `Resource` trait

This is the central abstraction. A type that implements `Resource` declares everything the framework needs to generate CRUD views for that entity.

```rust
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

/// An entity that can be managed through the dashboard.
/// E: the underlying entity type (row from the database).
#[async_trait]
pub trait Resource: Send + Sync + 'static {
    /// The entity this resource manages.
    type Entity: Serialize + DeserializeOwned + Send + Sync + Clone + 'static;

    /// Unique key for this resource (used in URLs, permission strings, nav).
    fn key(&self) -> &'static str;

    /// Human-readable label for nav and page titles.
    fn label(&self) -> &'static str;

    /// Icon name (maps to an icon set; framework-agnostic identifier).
    fn icon(&self) -> &'static str { "cube" }

    /// Fields that define the shape of this entity in the UI.
    fn fields(&self) -> Vec<Field<Self::Entity>>;

    /// Which fields appear as columns in the list view, in order.
    fn list_columns(&self) -> Vec<&'static str>;

    /// Default sort for the list view.
    fn default_sort(&self) -> Sort { Sort::desc("created_at") }

    /// Fields that are searchable via the global search bar.
    fn search_fields(&self) -> Vec<&'static str> { vec![] }

    /// Filters available in the list view sidebar.
    fn filters(&self) -> Vec<FilterDef> { vec![] }

    /// Relationships to other resources for the detail view tabs.
    fn relationships(&self) -> Vec<Relationship> { vec![] }

    /// Actions available on this resource (beyond CRUD).
    fn actions(&self) -> Vec<Box<dyn Action<Self::Entity>>> { vec![] }

    /// Metrics attached to this resource's detail page.
    fn metrics(&self) -> Vec<Box<dyn Metric>> { vec![] }

    /// The policy that governs access to this resource.
    fn policy(&self) -> &dyn Policy<Self::Entity>;

    /// Feature flag that gates this entire resource (None = always visible).
    fn flag(&self) -> Option<&'static str> { None }
}
```

### 2.2 The `Field` type

Fields are **values, not trait objects** — they're data that describes how to render and validate one piece of an entity. No boxing needed.

```rust
pub struct Field<E> {
    /// Field name (matches the entity's serialized JSON key).
    pub name: &'static str,

    /// Display label.
    pub label: &'static str,

    /// Field type, which determines renderer + validation.
    pub kind: FieldKind,

    /// Whether this field appears in the list view.
    pub show_in_list: bool,

    /// Whether this field appears on the detail view.
    pub show_in_detail: bool,

    /// Whether this field appears in create/edit forms.
    pub show_in_form: bool,

    /// Whether this field is required in forms.
    pub required: bool,

    /// Whether this field is sortable in the list view.
    pub sortable: bool,

    /// Whether this field is searchable.
    pub searchable: bool,

    /// Roles that can see this field. Empty = all roles.
    pub visible_to: Vec<&'static str>,

    /// Roles that can edit this field. Empty = all roles with edit access.
    pub editable_by: Vec<&'static str>,

    /// Feature flag that gates this field (None = always visible).
    pub flag: Option<&'static str>,

    /// Custom validator (beyond required/kind-based validation).
    pub validator: Option<fn(&E) -> Result<(), String>>,

    _marker: std::marker::PhantomData<E>,
}

pub enum FieldKind {
    Text,
    Textarea,
    Richtext,
    Number,
    Currency,
    Boolean,
    Select { options: Vec<(&'static str, &'static str)> },
    MultiSelect { options: Vec<(&'static str, &'static str)> },
    Date,
    DateTime,
    Email,
    File { accept: Option<&'static str> },
    Image { accept: Option<&'static str> },
    Relation { resource_key: &'static str, display_field: &'static str },
    Badge { options: Vec<(&'static str, &'static str)> },
    Json,
    Computed { render: fn(&E) -> String },
}
```

### 2.3 The `DataAdapter` trait

Bridges the resource engine to the actual data store. One implementation per entity type, typically backed by SQLx for the reference implementation.

```rust
#[async_trait]
pub trait DataAdapter<E: Send + Sync>: Send + Sync {
    /// The primary key type for this entity.
    type Id: Serialize + DeserializeOwned + Send + Sync + std::fmt::Display;

    /// Paginated, filtered, sorted list.
    async fn list(
        &self,
        page: usize,
        per_page: usize,
        sort: &Sort,
        filters: &[Filter],
        search: Option<&str>,
    ) -> Result<PaginatedResult<E>, DataError>;

    /// Single record by id.
    async fn get(&self, id: &Self::Id) -> Result<Option<E>, DataError>;

    /// Create a new record from form data.
    async fn create(&self, data: serde_json::Value, actor: &Actor) -> Result<E, DataError>;

    /// Update an existing record with a patch.
    async fn update(&self, id: &Self::Id, patch: serde_json::Value, actor: &Actor) -> Result<E, DataError>;

    /// Delete a record.
    async fn delete(&self, id: &Self::Id, actor: &Actor) -> Result<(), DataError>;

    /// Count records matching filters (for metrics).
    async fn count(&self, filters: &[Filter]) -> Result<u64, DataError>;

    /// Aggregate query (for metric queries).
    async fn aggregate(&self, agg: &Aggregation) -> Result<serde_json::Value, DataError>;
}

pub struct PaginatedResult<E> {
    pub items: Vec<E>,
    pub total: u64,
    pub page: usize,
    pub per_page: usize,
}

#[derive(Debug)]
pub enum DataError {
    NotFound,
    Unauthorized,
    Validation(String),
    Internal(Box<dyn std::error::Error + Send + Sync>),
}
```

### 2.4 The `Policy` trait

```rust
pub trait Policy<E>: Send + Sync {
    /// Can the actor view this specific record?
    fn can_view(&self, actor: &Actor, record: &E) -> bool;

    /// Can the actor create new records of this type?
    fn can_create(&self, actor: &Actor) -> bool;

    /// Can the actor update this specific record?
    fn can_update(&self, actor: &Actor, record: &E) -> bool;

    /// Can the actor delete this specific record?
    fn can_delete(&self, actor: &Actor, record: &E) -> bool;
}

/// A default policy that denies everything except for the admin role.
/// Users implement this trait to define per-resource access rules.
pub struct DenyAll;

impl<E> Policy<E> for DenyAll {
    fn can_view(&self, _: &Actor, _: &E) -> bool { false }
    fn can_create(&self, _: &Actor) -> bool { false }
    fn can_update(&self, _: &Actor, _: &E) -> bool { false }
    fn can_delete(&self, _: &Actor, _: &E) -> bool { false }
}
```

### 2.5 The `Action` trait

```rust
#[async_trait]
pub trait Action<E: Send + Sync>: Send + Sync {
    fn key(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn scope(&self) -> ActionScope;
    fn requires_confirmation(&self) -> bool { false }
    fn input_fields(&self) -> Vec<ActionField>;
    fn policy(&self) -> &'static str; // permission string, e.g. "doctors.approve"
    fn flag(&self) -> Option<&'static str> { None }

    /// Execute the action against a record, returning a user-visible result.
    async fn execute(&self, record: &mut E, actor: &Actor, input: serde_json::Value) -> Result<ActionResult, ActionError>;
}

pub enum ActionScope {
    Record,
    Bulk,
    Standalone,
}

pub struct ActionField {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub required: bool,
}

pub enum ActionResult {
    Success { message: String },
    Redirect { url: String },
}

pub enum ActionError {
    Forbidden,
    Validation(String),
    Internal(Box<dyn std::error::Error + Send + Sync>),
}
```

### 2.6 The `Actor` type

Carved out early because it threads through every policy check, audit entry, and flag resolution.

```rust
pub struct Actor {
    pub id: String,
    pub email: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,  // "resource.action" pairs, expanded from roles
    pub team_id: Option<String>,   // for multi-tenant row scoping
}
```

---

## 3. Template engine choice

### 3.1 Decision: Tera

The framework's built-in templates (list view, detail view, create/edit forms, dashboard, nav, audit log) ship as **Tera** templates.

**Why Tera over Askama:**

| Concern                      | Tera                                          | Askama                                   |
| ---------------------------- | --------------------------------------------- | ---------------------------------------- |
| Template iteration           | Edit `.tera` file, refresh browser, done      | Recompile entire crate                   |
| User template overrides      | User drops a `.tera` file in a directory      | User must recompile with their template  |
| Dynamic field rendering      | `{% for field in fields %}` — just works      | Needs all field types known at compile time |
| Type safety                  | Runtime errors for missing variables          | Compile-time errors                      |

The killer requirement is **dynamic field rendering**. The table component iterates over `fields()` at runtime — the set of columns depends on the resource definition, which is code, not a template. Askama can't iterate over a runtime `Vec<Field>` without every possible field variant being a known type. Tera's dynamic nature is an asset here, not a liability.

The downside (runtime template errors) is mitigated by:
- Framework-owned templates are tested in CI (render every built-in template against known data).
- User overrides are opt-in — the default templates are correct by construction.

### 3.2 Template inventory (built-in)

| Template                 | Renders                                        |
| ------------------------ | ---------------------------------------------- |
| `resource/list.html.tera`  | Paginated table with search, filter, sort     |
| `resource/detail.html.tera`| Record fields + relationship tabs + actions   |
| `resource/form.html.tera`  | Create/edit form, field-by-field              |
| `dashboard/home.html.tera` | Metric cards grid                             |
| `layout/base.html.tera`    | Shell: nav, sidebar, user menu, breadcrumbs   |
| `layout/nav.html.tera`     | Sidebar navigation, resource list             |
| `audit/list.html.tera`     | Audit log table (global)                      |
| `audit/detail.html.tera`   | Per-record audit history tab                  |
| `users/login.html.tera`    | Login form                                    |
| `users/invite.html.tera`   | Invitation flow                               |
| `flags/list.html.tera`     | Feature flags management                      |

### 3.3 Tera custom functions and filters

Tera supports registering custom functions. These bridge the template to framework internals:

- `can(actor, permission)` → bool — RBAC check in templates
- `flag(actor, "flag_name")` → bool — flag check in templates
- `format_field(value, field_kind)` → safe HTML — field rendering with appropriate escaping
- `metric_value(metric_key)` → rendered metric HTML

---

## 4. Axum integration

### 4.1 Router construction

The framework's main entry point: given a set of `Resource` impls, construct an axum `Router` with all CRUD routes, RBAC guards, and templates wired.

```rust
use axum::Router;

pub struct Twentytoo {
    resources: Vec<Box<dyn Resource>>,
    pages: Vec<Box<dyn Page>>,
    auth_provider: Box<dyn AuthProvider>,
    template_engine: Tera,
    // ...
}

impl Twentytoo {
    pub fn builder() -> TwentytooBuilder { ... }

    pub fn into_router(self) -> Router {
        let mut router = Router::new();

        // Auth routes (login, logout, reset-password, invite)
        router = router.merge(auth_routes(&self.auth_provider));

        // Dashboard home
        router = router.route("/", get(dashboard_home));

        // Generated CRUD routes for each resource
        for resource in &self.resources {
            let prefix = format!("/{}", resource.key());
            let resource_router = resource_routes(resource.as_ref(), &self.template_engine)
                .route_layer(middleware::from_fn_with_state(
                    app_state.clone(),
                    rbac_guard,
                ));
            router = router.nest(&prefix, resource_router);
        }

        // Custom pages
        for page in &self.pages {
            router = router.route(page.route(), get(page.handler()));
        }

        // Built-in resource routes (users, flags, roles, audit-log)
        router = router.merge(users_module(&self.auth_provider));
        router = router.merge(flags_module());
        router = router.merge(audit_module());

        // Global middleware stack
        router = router.layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(SessionLayer::new())
                .layer(CompressionLayer::new())
        );

        router
    }
}
```

### 4.2 Generic CRUD handlers

The handlers are generic over `Resource` + `DataAdapter`. One implementation handles *every* resource's list/detail/create/edit/delete — no code generation, no per-resource handler boilerplate.

```rust
/// GET /stores?page=1&sort=name&status=active
async fn list_handler<R: Resource>(
    State(state): State<AppState>,
    Extension(actor): Extension<Actor>,
    Path(resource_key): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Html<String>, AppError> {
    let resource = state.registry.get(&resource_key)?;

    // RBAC: can the actor view this resource at all?
    if !resource.policy().can_view_any(&actor) {
        return Err(AppError::Forbidden);
    }

    // Resolve flag — if the resource is flagged off, 404
    if let Some(flag) = resource.flag() {
        if !state.flags.resolve(flag, &actor) {
            return Err(AppError::NotFound);
        }
    }

    let result = resource.adapter().list(
        params.page.unwrap_or(1),
        params.per_page.unwrap_or(25),
        &params.sort.unwrap_or_else(|| resource.default_sort()),
        &params.filters(),
        params.search.as_deref(),
    ).await?;

    let mut ctx = tera::Context::new();
    ctx.insert("resource", &ResourceViewModel::from(resource.as_ref()));
    ctx.insert("items", &result.items);
    ctx.insert("pagination", &result);
    ctx.insert("actor", &actor);

    let html = state.templates.render("resource/list.html.tera", &ctx)?;
    Ok(Html(html))
}
```

### 4.3 Middleware stack

Applied from outside-in:

```
Incoming Request
    |
    v
TraceLayer              # request tracing + span
    |
    v
CompressionLayer        # response compression
    |
    v
SessionLayer            # session cookie → Actor extraction
    |
    v
AuditLayer              # logs mutation requests (POST/PUT/DELETE) to audit store
    |
    v
RbacGuard               # route-level permission check (rejects early)
    |
    v
FlagGuard               # checks resource/action flags, returns 404 if off
    |
    v
Handler                 # resource CRUD or custom page handler
```

### 4.4 Application state

```rust
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ResourceRegistry>,
    pub templates: Arc<Tera>,
    pub flags: Arc<FlagService>,
    pub audit: Arc<AuditService>,
    pub db: Arc<sqlx::PgPool>,          // or a generic DataStore handle
    pub session_store: Arc<SessionStore>,
}
```

`ResourceRegistry` maps resource keys to `&dyn Resource` and holds the `DataAdapter` for each.

---

## 5. Data adapter — reference implementation (SQLx + PostgreSQL)

The first `DataAdapter` implementation targets PostgreSQL via SQLx. This is the reference, not the only option — the trait is designed for swappability.

### 5.1 Approach

**Use SQLx's compile-time checked queries** (`sqlx::query_as!`) for type safety. The adapter translates `Filter`, `Sort`, and pagination into SQL dynamically (offset/limit and WHERE clauses), but the core query shape is checked at compile time.

### 5.2 Dynamic query building

The tension: SQLx's `query_as!` macro requires a static query string, but list queries need dynamic WHERE/ORDER BY/LIMIT. Resolution: use SQLx's runtime query API (`sqlx::query_as::<_, E>`) with bind parameters. The type mapping from Rust struct to row is still checked — we lose the compile-time SQL validation for dynamic clauses, but gain the flexibility to build queries from runtime `Filter` and `Sort` parameters.

Alternative considered: `sea-query` for building queries programmatically with type safety. Adds a dependency but avoids hand-building SQL strings. Worth evaluating; not a hard requirement for MVP.

```rust
pub struct SqlxAdapter<E> {
    pool: sqlx::PgPool,
    table: &'static str,
    _marker: std::marker::PhantomData<E>,
}

#[async_trait]
impl<E: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Sync + Unpin>
    DataAdapter<E> for SqlxAdapter<E>
{
    type Id = String; // or a generic, depending on entity

    async fn list(
        &self,
        page: usize,
        per_page: usize,
        sort: &Sort,
        filters: &[Filter],
        search: Option<&str>,
    ) -> Result<PaginatedResult<E>, DataError> {
        let mut query = format!("SELECT * FROM {}", self.table);
        let mut conditions: Vec<String> = Vec::new();
        let mut bind_idx = 1u32;

        // Apply filters
        for filter in filters {
            // ... build WHERE clauses with parameterized values
        }

        // Apply search
        if let Some(term) = search {
            // ... ILIKE across search_fields
        }

        // Count total
        let count_sql = format!("SELECT COUNT(*) FROM {} {}", self.table, where_clause);
        let total: (i64,) = sqlx::query_as(&count_sql)
            .fetch_one(&self.pool).await?;

        // Apply sort + pagination
        let offset = (page - 1) * per_page;
        let data_sql = format!(
            "SELECT * FROM {} {} ORDER BY {} LIMIT $1 OFFSET $2",
            self.table, where_clause, sort.to_sql()
        );

        let items: Vec<E> = sqlx::query_as(&data_sql)
            .bind(per_page as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool).await?;

        Ok(PaginatedResult { items, total: total.0 as u64, page, per_page })
    }
    // ...
}
```

---

## 6. Template rendering and progressive enhancement

### 6.1 HTML over the wire

Every response from generated handlers returns `Html(String)` — fully rendered server-side. No JSON API for resource CRUD (the "API" is the form POST that returns a redirect + flash message, or the list view re-rendered with new params).

### 6.2 Progressive enhancement with htmx

The built-in templates include `htmx` attributes on interactive elements:

- **Sortable column headers:** `hx-get="/stores?sort=name" hx-target="#resource-table" hx-swap="outerHTML"`
- **Filter form:** `hx-get="/stores" hx-target="#resource-table" hx-trigger="change"` (auto-submit on filter change)
- **Search:** `hx-get="/stores" hx-target="#resource-table" hx-trigger="keyup changed delay:300ms"`
- **Pagination:** `hx-get="/stores?page=2" hx-target="#resource-table"`
- **Inline edit toggle:** `hx-get="/stores/42/edit" hx-target="#detail-card"`

With JS disabled, these degrade to full-page navigation (the `hx-*` attributes are ignored, the `<a>`/`<form>` still work as standard HTTP). With htmx loaded (a single `<script>` tag), the experience upgrades to partial-page swaps.

The framework ships htmx (~14KB minified + gzipped) as a static asset. Users can opt out entirely, swap it for unpoly or Turbo, or add Alpine.js for client-side state on custom pages — nothing in the framework depends on a specific JS library.

### 6.3 The `Page` trait (custom page escape hatch)

```rust
#[async_trait]
pub trait Page: Send + Sync {
    fn route(&self) -> &'static str;
    fn nav_label(&self) -> &'static str;
    fn policy(&self) -> &'static str;
    fn flag(&self) -> Option<&'static str> { None }

    async fn handler(
        &self,
        ctx: PageContext,
    ) -> Result<axum::response::Response, AppError>;
}

pub struct PageContext {
    pub actor: Actor,
    pub templates: Arc<Tera>,
    pub db: Arc<sqlx::PgPool>,
    pub request: axum::extract::Request,
}
```

Custom pages get the full axum `Request` and can return any `IntoResponse`. They can compose built-in UI primitives (table component, form component) through the template engine, or return completely custom HTML. The escape hatch is wide open — it's an axum handler with framework conveniences, not a constrained DSL.

---

## 7. The declarative surface — builder vs. macros

### 7.1 Builder pattern (v1 approach)

Resources are defined with a builder, not a proc macro or custom DSL. This keeps the surface pure Rust — standard tooling (rust-analyzer, fmt, clippy) works without configuration.

```rust
use twentytoo::prelude::*;

struct Store {
    id: String,
    name: String,
    owner_id: String,
    status: StoreStatus,
    created_at: chrono::DateTime<chrono::Utc>,
}

struct StoreResource;

impl Resource for StoreResource {
    type Entity = Store;

    fn key(&self) -> &'static str { "stores" }
    fn label(&self) -> &'static str { "Stores" }
    fn icon(&self) -> &'static str { "storefront" }

    fn fields(&self) -> Vec<Field<Store>> {
        fields!(
            field!("name", "Store Name", Text, required: true, list: true, form: true, sortable: true, searchable: true),
            field!("owner_email", "Owner", Email, list: true, form: true, searchable: true),
            field!("status", "Status", Badge {
                options: &[("pending", "Pending"), ("active", "Active"), ("suspended", "Suspended")]
            }, list: true, form: true),
            field!("created_at", "Created", DateTime, list: true, sortable: true, form: false),
        )
    }

    fn list_columns(&self) -> Vec<&'static str> {
        vec!["name", "owner_email", "status", "created_at"]
    }

    fn default_sort(&self) -> Sort { Sort::desc("created_at") }

    fn policy(&self) -> &dyn Policy<Store> {
        &StorePolicy
    }
}
```

The `field!` macro (a simple `macro_rules!`, not a proc macro) reduces boilerplate by filling in default `false` for unspecified options.

### 7.2 Future: derive macro

Once the builder pattern stabilizes, a derive macro can collapse the common case:

```rust
#[derive(Resource)]
#[resource(key = "stores", label = "Stores", icon = "storefront")]
#[policy(StorePolicy)]
struct Store {
    #[field(label = "Store Name", list, form, searchable)]
    name: String,

    #[field(label = "Owner", list, form, searchable)]
    owner_email: String,

    #[field(label = "Status", list, form, kind = "badge")]
    #[badge_options(pending = "Pending", active = "Active", suspended = "Suspended")]
    status: StoreStatus,

    #[field(label = "Created", list, sortable)]
    created_at: chrono::DateTime<chrono::Utc>,
}
```

This is nice-to-have, not MVP. The builder is the source of truth; the derive macro is sugar that generates the builder calls. The derive never adds capability that the builder doesn't have — it's strictly a reduction in boilerplate.

### 7.3 Rejected: custom DSL with proc macro

A proc macro that parses a custom DSL (like the pseudocode in the core spec) was considered and rejected for v1. The cost:
- No rust-analyzer support (no completions, no inline errors, no go-to-definition)
- Every change to the DSL grammar is a proc macro change — harder to iterate
- Debugging proc macros is painful (cargo expand, opaque error spans)
- Users already know Rust — they don't need to learn a new mini-language

The builder pattern gives the same declarative feel with full IDE support.

---

## 8. Dependency decisions

### 8.1 Core dependencies

| Dependency       | Purpose                                      | Rationale                                      |
| ---------------- | -------------------------------------------- | ---------------------------------------------- |
| `axum` 0.8       | HTTP framework, routing, extractors          | First-class Tower integration, maintained by tokio team |
| `tokio` 1        | Async runtime                                | Standard for Rust async; axum requires it      |
| `tower` 0.5      | Middleware abstraction                       | Composable layer stack; axum is built on it    |
| `tower-http` 0.6 | CORS, tracing, compression, sessions         | Battle-tested middleware, no reason to reinvent |
| `tera` 1         | Template engine                              | Dynamic rendering, user-overridable templates  |
| `sqlx` 0.8       | PostgreSQL driver with compile-time checks   | Type-safe queries, async, migrations included  |
| `serde` / `serde_json` | Serialization                          | Ubiquitous; required by axum's `Json` extractor |
| `chrono`         | Date/time types                              | Needed for DateTime fields, audit timestamps   |
| `async-trait`    | Async fn in traits                           | Required until Rust natively stabilizes AFIT in more positions |
| `argon2`         | Password hashing                             | Standard for user auth                         |
| `rand`           | Token generation, invite codes               | Standard                                       |
| `tracing` + `tracing-subscriber` | Structured logging               | Standard for tokio ecosystem                   |
| `uuid`           | Entity IDs, audit entry IDs                  | Standard                                       |

### 8.2 Avoided dependencies

| Dependency       | Why avoided                                  |
| ---------------- | -------------------------------------------- |
| `askama`         | Requires compile-time templates; conflicts with dynamic field rendering |
| `diesel`         | Compile-time query building is too rigid for dynamic filter/sort |
| `sea-orm`        | Heavier than SQLx for the adapter pattern; more opinionated |
| `actix-web`      | axum's Tower integration is a better fit for the middleware stack |
| `yew` / `leptos` | SSR-only; no WASM framework needed           |
| `jsonwebtoken`   | Session-based auth for v1; JWT adds complexity without benefit for internal tools |

### 8.3 Conditional / deferred dependencies

| Dependency       | When it's added                              |
| ---------------- | -------------------------------------------- |
| `sea-query`      | When hand-building SQL strings becomes painful (likely Phase 2) |
| `tokio-tungstenite` | When WebSocket/SSE live metrics are added (Phase 4) |
| `lettre`         | When email notifications ship (Phase 3)      |
| `oauth2`         | When SSO/OIDC auth provider is added (post-Phase 1) |
| `ical` / `cron`  | If scheduled actions are added               |

---

## 9. Module registration pattern

Modules (users, flags, custom domain modules) register with the framework at startup via a `Module` trait:

```rust
#[async_trait]
pub trait Module: Send + Sync {
    fn name(&self) -> &'static str;

    /// Resources this module contributes.
    fn resources(&self) -> Vec<Box<dyn Resource>> { vec![] }

    /// Custom pages this module contributes.
    fn pages(&self) -> Vec<Box<dyn Page>> { vec![] }

    /// Axum routes this module contributes (for non-resource, non-page endpoints).
    fn routes(&self) -> Option<axum::Router> { None }

    /// Database migrations this module requires.
    fn migrations(&self) -> Vec<Migration> { vec![] }

    /// Called at startup — module can initialize connections, warm caches, etc.
    async fn init(&self, ctx: &ModuleContext) -> Result<(), ModuleError>;
}
```

The consuming application's `main.rs`:

```rust
#[tokio::main]
async fn main() {
    let app = Twentytoo::builder()
        .with_module(UsersModule::new())
        .with_module(FlagsModule::new())
        .with_module(StoresModule::new())    // domain-specific
        .with_module(DoctorsModule::new())   // domain-specific
        .with_database(&std::env::var("DATABASE_URL").unwrap())
        .with_template_dir("templates/")     // framework + user overrides
        .with_theme(ThemeTokens::default())
        .build()
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app.into_router()).await.unwrap();
}
```

---

## 10. Open design questions (Rust-specific)

### 10.1 `dyn Resource` vs. generics

The `ResourceRegistry` stores `Vec<Box<dyn Resource>>` — trait objects. This is ergonomic (heterogeneous collection of different resource types) but has costs: dynamic dispatch on every call, and the `Resource` trait must be object-safe.

Alternative: an enum-based approach where each resource variant is a known enum member. This is static dispatch but requires the framework to know every resource type at compile time — conflicts with the module system where third-party modules contribute unknown resource types.

**Leaning:** trait objects for the registry. The performance cost of dynamic dispatch on `can_view()` or `fields()` is negligible compared to the database round-trip that follows. If profiling shows it matters, the hot path (list handler) can monomorphize over the resource type after the initial lookup.

### 10.2 `Send + Sync` everywhere

Axum 0.8 requires handlers to be `Send + Sync`. Every trait (`Resource`, `DataAdapter`, `Policy`, `Action`, `Module`) must propagate these bounds. This is noisy but unavoidable — the alternative (using `Arc<Mutex<>>` wrappers) is worse. The bounds are a one-time cost in trait definitions; downstream implementors inherit them automatically.

### 10.3 Error type strategy

`AppError` is the framework's error type — it implements `IntoResponse` and maps to HTTP status codes + user-visible messages.

```rust
pub enum AppError {
    NotFound,
    Forbidden,
    FlaggedOff,         // resource gated behind an inactive flag
    Unauthorized,
    Validation(HashMap<String, String>),  // field_name → error message
    Data(DataError),
    Template(TeraError),
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden").into_response(),
            AppError::FlaggedOff => (StatusCode::NOT_FOUND, "Not found").into_response(), // don't leak flag state
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
            AppError::Validation(errors) => {
                // Return JSON so htmx can display field-level errors
                (StatusCode::UNPROCESSABLE_ENTITY, Json(errors)).into_response()
            }
            AppError::Data(e) => match e {
                DataError::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
                DataError::Unauthorized => (StatusCode::FORBIDDEN, "Forbidden").into_response(),
                DataError::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response(),
                DataError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response(),
            },
            AppError::Template(e) => {
                tracing::error!(?e, "Template rendering failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
            }
            AppError::Internal(e) => {
                tracing::error!(?e, "Internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
            }
        }
    }
}
```

Note: `FlaggedOff` → 404, not 403. Don't leak the existence of a flagged resource.

### 10.4 Template hot-reloading in development

Tera loads templates from disk. In development, `Tera::full_reload()` on every request would be slow. Instead, use file-system watching (via `notify` crate) to reload changed templates. In production, templates are loaded once at startup.

### 10.5 Migrations

SQLx supports migrations. The framework provides its own migrations for built-in tables (`users`, `sessions`, `roles`, `permissions`, `flags`, `audit_log`). Domain modules contribute their own migrations. A `twentytoo migrate` subcommand runs all pending migrations in order.

### 10.6 Session store

Internal tools don't need distributed sessions. For MVP: server-side session store in PostgreSQL (a `sessions` table with expiry). The session cookie is a random token; session data (actor, roles, permissions) is loaded from the DB on each request (cached in the `Extension<Actor>` layer). For higher scale: pluggable `SessionStore` trait with a Redis implementation later.

---

## 11. Implementation order (Rust-specific)

The sequence from `00-init.md` Section 15, translated to concrete Rust artifacts:

### Step 1 — Scaffold workspace
- `Cargo.toml` workspace, `crates/twentytoo-core/`, `crates/twentytoo/`, `examples/demo/`
- CI with `cargo test`, `cargo clippy`, `cargo fmt --check`

### Step 2 — `twentytoo-core` traits + types
- `Resource`, `Field`, `DataAdapter`, `Policy`, `Action`, `Actor`, `AppError`, `PaginatedResult`
- No database, no HTTP — just the type system

### Step 3 — SQLx adapter for one entity
- `SqlxAdapter` implementing `DataAdapter` for a `User` entity against PostgreSQL
- Prove the trait works: list, get, create, update, delete with filtering and pagination

### Step 4 — Tera templates + axum handlers
- `resource/list.html.tera`, `resource/detail.html.tera`, `resource/form.html.tera`
- Generic list/detail/create/edit handlers
- `UserResource` in the demo app — manually walk through every generated view in a browser

### Step 5 — Auth + session
- `AuthProvider` trait, password-based implementation
- Login/logout/invite flow, session cookie → `Actor` extraction middleware

### Step 6 — RBAC
- `roles` and `permissions` tables, migration
- `Policy` trait wired into handlers
- `RbacGuard` middleware for route-level enforcement
- Template `can()` function for UI-level enforcement

### Step 7 — Audit log
- `audit_log` table, migration
- `AuditLayer` middleware logs every mutation
- Audit log resource (itself RBAC-gated) — proves the framework works on itself

### Step 8 — Feature flags
- `flags` table, migration
- `FlagService` with targeting strategies
- `FlagGuard` middleware
- Template `flag()` function
- `flag: "..."` integration on resources, fields, actions

### Step 9 — Second resource + module system
- Define a `StoreResource` in the demo app — this proves the resource engine isn't hardcoded to Users
- Extract the module registration pattern
- Users and Flags become built-in modules registered like any other

### Step 10 — Actions + metrics
- `Action` trait wired to buttons on detail view + list toolbar
- `Metric` trait wired to dashboard home + resource detail pages

---

## 12. Tera template snippet (example)

To ground the template discussion in real code, here's how the generated list view iterates over resource fields:

```html
{# resource/list.html.tera #}
{% extends "layout/base.html.tera" %}

{% block content %}
<div class="resource-header">
    <h1>{{ resource.label }}</h1>
    {% if can(actor, resource.key ~ ".create") and flag(actor, resource.flag) %}
        <a href="/{{ resource.key }}/new" class="btn btn-primary">New {{ resource.label }}</a>
    {% endif %}
</div>

{# Search + filter bar #}
<form hx-get="/{{ resource.key }}" hx-target="#resource-table" hx-trigger="submit">
    <input type="search" name="search" placeholder="Search {{ resource.label }}..."
           hx-get="/{{ resource.key }}" hx-target="#resource-table"
           hx-trigger="keyup changed delay:300ms" />
    {% for filter in resource.filters %}
        {{ format_filter(filter)|safe }}
    {% endfor %}
</form>

{# Table #}
<div id="resource-table">
    <table>
        <thead>
            <tr>
                {% for col in resource.list_columns %}
                    <th hx-get="/{{ resource.key }}?sort={{ col }}" hx-target="#resource-table">
                        {{ col }}
                    </th>
                {% endfor %}
                <th>Actions</th>
            </tr>
        </thead>
        <tbody>
            {% for item in items %}
                <tr>
                    {% for col in resource.list_columns %}
                        <td>{{ format_field(item[col], resource.fields[col].kind)|safe }}</td>
                    {% endfor %}
                    <td>
                        {% for action in resource.actions %}
                            {% if can(actor, action.policy) and flag(actor, action.flag) %}
                                <button hx-post="/{{ resource.key }}/{{ item.id }}/actions/{{ action.key }}">
                                    {{ action.label }}
                                </button>
                            {% endif %}
                        {% endfor %}
                    </td>
                </tr>
            {% endfor %}
        </tbody>
    </table>

    {# Pagination #}
    {% include "partials/pagination.html.tera" %}
</div>
{% endblock %}
```

Key design points:
- `format_field()` is a Tera custom function that renders a value based on its `FieldKind` — a Badge renders as a colored pill, a Relation renders as a link, an Image renders as an `<img>` tag. The function returns pre-escaped HTML (marked safe in Tera).
- `can()` and `flag()` are also custom functions — they check the current `Actor`'s permissions and the flag service, respectively.
- htmx attributes are in the *framework's* templates, not the user's code. Users who don't want htmx can override these templates entirely.
- The table is a single `<div>` target so htmx swaps the whole table on sort/filter/page changes.

---

## 13. What this is not

- **Not a Rust ORM.** Twentytoo doesn't try to abstract over databases — it uses SQLx directly for the reference implementation and provides the `DataAdapter` trait for users who want to plug in their own data layer.
- **Not a build tool.** There's no `twentytoo new` or `twentytoo generate` scaffolding command (yet). The consuming app is a standard Cargo project that depends on `twentytoo` as a library.
- **Not a proc-macro framework.** The v1 surface is traits and builders. Macros are a future optimization, not a design requirement.
- **Not WASM-dependent.** The dashboard renders on the server. The only JS is the optional htmx script for progressive enhancement — no WASM, no npm, no bundler.
