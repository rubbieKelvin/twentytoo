# 00-architecture.md — Twentytoo: the consolidated design record

**Working name:** Twentytoo
**Status:** Live design record — consolidated from the former 00/01/02/03/05 brainstorm docs (2026-08-14). Everything here is either implemented in the workspace or an explicitly named deferred slice; concepts that never made it into the development are dropped (see §12).
**Reference language:** Rust. **Template engine:** MiniJinja.

---

## 1. Purpose

Every engineering team that operates a non-trivial product eventually needs internal tools — a support dashboard, an ops panel, an approval queue. And every team solves the same problems from scratch: auth, RBAC, tables with filtering/sorting/pagination, forms with validation, audit logging. The result is a proliferation of half-built, under-permissioned admin panels.

Twentytoo exists to make that class of work **declarative rather than imperative**. Instead of building CRUD screens for your Nth internal entity, you define the entity — its fields, its policy, its data adapter — and the framework generates the rest.

The measure of success: standing up a new internal tool should require **domain logic and resource definitions only** — never re-solving auth, RBAC, tables, forms, filtering, pagination, or audit logging.

## 2. What it is and what it isn't

**It is** a server-rendered, internal-tools dashboard framework: teams declare resources and the framework generates permissioned CRUD views. It ships as a **library**, not a template. you compose it into your app; upstream improvements are a dependency bump away.

**It is not:**

- **Not a no-code builder.** It is a framework for engineers; resources are defined in Rust code.
- **Not a BI/analytics platform.** No metric UI ships yet (§11); the aggregation contract exists to back one later.
- **Not a public-facing product UI toolkit.** Internal tools only.
- **Not a headless CMS** — the data model is operational entities.
- **Not a workflow/automation engine** — no multi-step approval chains, no DAGs.
- **Not a React/SPA stack** — HTML over the wire; the only client JS is the Tabler bundle (Bootstrap components) plus a tiny optional enhancement script.

## 3. Design principles

- **SSR-first, progressively enhanced.** The dashboard works with JS disabled; Tabler's components (modals, dropdowns, toasts) sit on top of plain HTTP forms and links — every navigation is a full page load, and list state (sort, filter, search, pagination) lives entirely in the URL (§8.6).
- **Convention over configuration for the 80% case; escape hatches for the 20%.** Anything that fits the resource model needs zero boilerplate. Anything that doesn't composes with plain axum: the built router can be nested into a larger app, and custom routes are ordinary axum handlers with access to the same state and templates.
- **Declarative, not generated.** A resource is defined once as a trait impl — there is no scaffolded code to drift out of sync. The definition *is* the source of truth.
- **RBAC and audit logging are first-class.** Every view and button renders or doesn't render based on policy; every mutation is logged (§6.5). A page that renders without checking permissions is a bug, not a missing feature.
- **Row-level scoping is part of the core policy model** (`Actor.team_id`, per-record policy methods), even if a deployment never uses it.
- **Conservative defaults.** `Policy` denies by default; `DataAdapter` defaults return `Unsupported`; capabilities default to a read-only baseline; an unset feature flag is off. Unconfigured means safe, never means permissive.
- **Fail at boot, not at first click.** Misconfiguration (a declared identifier missing from its source, a template name that doesn't resolve, a bad auth setup) fails `build()`, before the first request.
- **Boring where it counts.** Auth, sessions, CSRF, SQL injection, XSS — solved problems; battle-tested libraries only, no innovation on security plumbing.

## 4. Architecture: three crates, one data flow

A three-crate Cargo workspace (`Cargo.toml`, resolver 3, edition 2024, MSRV 1.94):

| Crate | Owns | Runtime |
| --- | --- | --- |
| `crates/twentytoo-core` | The contract: traits and types only — `Resource`, `Field`, `DataAdapter`, `Policy`, `Actor`, `Action`/`Aggregation`/`AuditEvent` contract types, the query/write/capability models, and the `InMemoryAdapter` reference implementation | No tokio, no HTTP, no IO |
| `crates/twentytoo` | The HTTP layer: the builder, generic axum handlers, the MiniJinja template engine, auth, middleware, view models, errors | axum + tokio |
| `crates/twentytoo-db` | The PostgreSQL layer: the embedded schema and typed access for the framework-owned tables (users, sessions, login tokens, groups, roles, permissions, the `inapp_events` audit stream) | sqlx 0.8 |

Data flow:

1. A `Resource` impl declares its `Entity`, `fields()`, `policy()`, and `adapter() -> Arc<dyn DataAdapter<Entity>>`.
2. The engine builds a `Query` — the **policy scope is already merged into the query filter; adapters never see policies**.
3. Writes arrive as `Mutation<Id>` (Create/Update/Delete/Upsert) wrapped in `WriteContext` (expected version, idempotency key, actor escape hatch).
4. `capabilities()` is read once at boot; the UI degrades to the declared capability grade (§5.6).
5. Entities travel to the view layer only as serialized JSON — typed entities are an adapter-side optimization, never a framework requirement.

## 5. The core contract (`twentytoo-core`)

### 5.1 Resource

`Resource` is the central abstraction: one browsable, searchable, actionable surface over one entity and one data source. It declares:

- `key()` / `label()` / `icon()` — stable key used in URLs and permission strings, human label, nav icon.
- `fields()` — all fields of the entity, in definition order (§5.2).
- `list_columns()`, `default_sort()` (defaults to `created_at` desc), `search_fields()`, `filters()` (`FilterSpec`: field + operator + optional sidebar label), `relationships()` (`Relationship`: tab key/label, related resource key, back-reference field — declared but not yet rendered, §11).
- `actions()` — custom actions (§5.11), default empty.
- `policy()` — the row-level access policy (§5.10).
- `flag()` — an optional feature-flag name gating the whole resource (§7.6).
- `adapter()` — the data source, built once where pools and clients live and shared as `Arc<dyn DataAdapter<Entity>>`.

`metrics()` is deliberately absent from the trait; it arrives as a defaulted method in the metrics slice (§11).

### 5.2 Field

Fields are **values, not trait objects**: data describing how to render and validate one piece of the entity. `Field<E>` carries name, label, and kind, plus booleans: `show_in_list`, `show_in_detail`, `show_in_form`, `required`, `sortable`, `searchable`. It also carries:

- `visible_to` / `editable_by` — role lists gating field-level visibility and editability; empty means everyone. Applied per actor in the view layer, before anything reaches a template.
- `flag` — feature-flag name gating the field.
- `validator` — an entity-level custom validator.

`FieldKind` is non-generic (it describes serialized JSON entities): `Text`, `Textarea`, `Richtext`, `Number`, `Currency`, `Boolean`, `Select`, `MultiSelect`, `Date`, `DateTime`, `Email`, `File`, `Image`, `Relation { resource_key, display_field }`, `Badge { options }`, `Json`, `Computed { render }`. (`FieldSpec` reuses it for `DataAdapter::describe`.)

Fields are declared with the `field!` / `fields!` macros (plain `macro_rules!`, not proc macros): kind argument as a bare ident or braced form, flags like `required: true` / `list: true` in any order, `visible_to`/`editable_by`/`flag`/`validator` via struct-update syntax.

### 5.3 The graded `DataAdapter`

`DataAdapter<E, Id = String>` is a **graded** contract — a source implements what it can honestly do, declares the grade, and the engine adapts:

- **Must implement:** `capabilities()`, `list(&Query) -> Page<E>`, `get(&Id)`.
- **Defaulted:** `get_many` (default: sequential `get`, preserving input order, skipping missing ids), `create` / `update` / `delete` (default: `Unsupported`), `apply_mutations` (default: sequential, stop at first error; `Upsert` retries `Conflict` as update), `begin()` (default: `Unsupported`), `aggregate` (default: `Unsupported`), `stream` (default: page through `list`), `describe` (default: `Unsupported`), `validate` (default: `Ok(())`).

Defaults are **conservative — unsupported operations return `DataError::Unsupported`, never panics or optimistic stubs.** `Id` is a defaulted generic, not an associated type, so `Arc<dyn DataAdapter<E>>` stays object-safe for the registry.

### 5.4 The query model and pages

One bounded struct family, translatable by every source:

- **`FilterNode`** — a tree: `Field { field, op, value }`, `And`, `Or`, `Not`. Operators: `Eq`, `Ne`, `Gt`, `Gte`, `Lt`, `Lte`, `In`, `NotIn`, `Contains`, `StartsWith`, `IsNull`, `IsNotNull`, `FullText`. Values are typed (`Null`, `Bool`, `Int`, `Float`, `Str`, `DateTime`, `In`, `Range`). List-view params build a tree (sidebar filters → `And`; date ranges → `Range`); the adapter flattens or nests as its source requires.
- **`SortField`** — multi-column sort: field, direction (`Asc`/`Desc`), nulls ordering (`First`/`Last`/`Default`).
- **`SearchSpec`** — a term across the listed fields; `SearchMode` (`None`/`Exact`/`Substring`/`FullText`) declares what search means for the source and which UX renders.
- **`Pagination`** — `Offset { page, per_page }` (1-based, numbered pager) or `Cursor { after, before, per_page }` (opaque, adapter-encoded, framework-blind cursors).
- **`Query`** — pagination, sort, filter (user filters ∧ policy scope, already merged), search, projection (column names or all fields).

**`Page<E>`** echoes the request pagination and returns `items`, `total: Option<u64>`, and optional `next`/`prev` cursors. The framework renders exactly one of two pagers:

- `total: Some(n)` → numbered pages;
- `total: None` → prev/next, driven purely by cursors.

### 5.5 Writes

- **`Mutation<Id>`** — `Create { data }`, `Update { id, patch }`, `Delete { id }`, `Upsert { id, data }` (create-or-update by id — the common re-import shape).
- **`WriteContext`** — `expected_version: Option<Version>` (optimistic concurrency: the adapter compares and fails with `Conflict`; the UI can then show "this record changed"), `idempotency_key` (HTTP header, idempotency column, or unique constraint — import retries), and `actor` as an escape hatch for the rare source that authenticates per-user. Most adapters ignore `actor`.
- **`Version`** — an opaque optimistic-concurrency token (DB row version / API etag).

### 5.6 Capabilities and honest degradation

`Capabilities` is the source's declared ability set: pagination modes, cheap totals, write grade (`ReadOnly` | `Crud` | `Bulk`), transactions, search mode, expressible filter operators, sort, aggregation grade (`None` | `Basic` | `Grouped` | `Histogram`), concurrency support (`None` | `Version` | `Etag`), native streaming, schema discovery. The default is a **read-only baseline; every upgrade is explicit**.

Read once at boot and cached in a per-resource feature matrix, the matrix drives the UI:

| Capability absent | Engine behavior |
| --- | --- |
| `write = ReadOnly` | No create/edit/delete buttons, no form routes — a browse/export surface |
| `totals = false` | Prev/next instead of numbered pages |
| `search = None` | Search box not rendered |
| `filter_ops` | Only controls the source can express are offered |
| `sort = false` | Sortable headers not rendered |
| `aggregation = None` | Metric cards not offered for this resource |

`DataError::Unsupported` remains as a defensive backstop for engine bugs; capabilities are the primary signaling mechanism. The same generic handler drives an offset source with numbered pages and a cursor-only source with prev/next.

### 5.7 Transactions

`TxAdapter<E, Id>` is a separate sub-trait (`get`, `apply(&mut self, mutations)`, `commit`, `rollback`) so the main trait stays object-safe and read-only adapters never see transaction machinery. `DataAdapter::begin()` defaults to `Unsupported`; without transactions the engine falls back to sequential mutations with per-row error reporting.

### 5.8 Aggregation

Typed, metric-shaped, in the contract even though no metric UI ships yet. `Aggregation { measure, group_by, filter, sort, limit }` with `Measure` (`Count`, `Sum`, `Avg`, `Min`, `Max`, `Distinct`) and `GroupBy` (`Field` or `DateHistogram { field, interval }`, intervals minute→year). Results are either a scalar value or buckets (`key`, `value`). Mapping is mechanical per source (SQL `GROUP BY date_trunc(…)`, ES `aggs`, …). The five classic metric shapes (`value`, `trend`, `partition`, `table`, `progress`) all reduce to these measures.

### 5.9 The `InMemoryAdapter` reference implementation

A complete HashMap-backed engine that proves the contract and powers the demo and the test suite: filter tree evaluation, offset + base64-cursor pagination, multi-column sort with nulls ordering, search, projection, writes with conflict/version semantics, transactions, aggregation, and streaming. Every trait default and every capability is exercised against it in CI — it is the reference implementation of the contract, and the proof that the contract is implementable as specified.

### 5.10 Policy and Actor

`Policy<E>` is per-resource authorization with **deny-by-default**: `can_view_any`, `can_view`, `can_create`, `can_update`, `can_delete` all default to `false`; `DenyAll` is the baseline. Record methods receive the entity and are called wherever the engine has the record in hand (detail view, mutation checks). Adapters never see policies — the engine merges the policy scope into the query filter, so `WHERE owner_id = $1` is just another `Eq` node.

`Actor` is the acting principal: `id`, `email`, `roles`, `permissions` (expanded `"resource.action"` codes), `team_id` (multi-tenant row scoping). `Actor::can(permission)` matches case-sensitively, two-segment, wildcard-per-segment: `*.view` matches `stores.view`, `stores.*` matches `stores.view`. It serializes, so the template layer can read it from the render context.

### 5.11 Actions — contract only

The contract types exist (`Action<E>` with `execute`, `ActionScope` `Record`/`Bulk`/`Standalone`, `ActionField`, `ActionResult`, `ActionError`) and `Resource::actions()` returns them, but **no HTTP surface renders or runs them yet** (§11).

### 5.12 Audit events — contract only

The audit trail is an append-only event stream (§6.5). `AuditEvent` (id, timestamp, `resource.action` type string, `actor`/`target` point-in-time `EventResource` snapshot envelopes, type-specific `properties` JSON, request `context` JSON) plus `AuditAction` (`Create`, `Update`, `Delete`, `Execute`, `Login`, `Logout`, `Impersonate`) — the closed writer-side union the `type` suffix draws from — are the core shape of the audit trail. The DB layer stores them; which writes produce them today is §6.5.

## 6. Identity: RBAC, auth, and audit

### 6.1 The RBAC model

- **User** — an authenticated principal (`users` table, `status` active/suspended, lowercase emails, argon2-hashed password).
- **Group** — a grouping boundary; users join via `group_members` (many-to-many).
- **Role** — a named bundle of permissions (`roles`, joined via `role_permissions`).
- **Permission** — a `resource.action` code (`permissions` table).

Grants: `user_roles` (a user holding a role — globally, `group_id NULL`, or scoped to a group) and `group_roles` (a role held by a group — every member inherits it in every context). `Db::load_actor` expands the union of all those grants' permissions into the `Actor` the request pipeline sees.

### 6.2 Permission codes and actor expansion

A permission code is exactly two non-empty `[a-z0-9_*]`-only segments joined by `.` (`stores.view`, `*.view`); the access layer validates the shape before insert (malformed → validation error, duplicate → conflict). Matching at check time is the same shape: case-sensitive, per-segment wildcards, never more than two segments.

### 6.3 Login flow and sessions

Auth is **enabled per app** via the builder (`.auth(AuthConfig)`); without it, requests carry a configured `default_actor` — useful for an unauthenticated internal dashboard.

The login flow is **email → (code, when configured) → password**:

- The email step: unknown email on an allowed domain self-creates an account (no password yet) and audits the create; known emails proceed. Each in-progress step is anchored by a short-lived, single-use token in the `login_tokens` table.
- The code step (only when `email_confirmation` is on): a 6-digit code is delivered through the `CodeSender` seam (default: `ConsoleCodeSender` prints to stdout — no mail infrastructure ships) and verified; five wrong codes lock the token.
- The password step: verify (or, for a fresh domain account, set — minimum 8 characters) and create the session, auditing the login.

Sessions are **server-side in PostgreSQL**: the cookie (`twentytoo_session`) carries a random token, the `sessions` table stores only its SHA-256 hash and expiry (default 7 days) — a leaked table never yields usable session credentials. Logout deletes the session, audits the sign-out, and clears the cookies.

A `BootstrapAdmin` (email/name/password, hashed before storage) is seeded at build time when configured — the framework's only self-creation path besides domain access.

### 6.4 The built-in users area

When auth is configured, the framework mounts its own permission-gated `/users` area (list, create, edit) — hand-written handlers over the auth service's database handle, not generated resource routes. The `"users"` resource key belongs to it; a workspace resource named `users` is a build error.

### 6.5 The audit log

`inapp_events` is the canonical, append-only event stream: `type` is an **open** `resource.action` discriminator (no `CHECK` set — types evolve additively), `actor` and `target` are point-in-time resource envelopes (`{"type": <kind>, "properties": {…}}`) so entries survive actor deletion and record renames, `properties` carries the type-specific payload (before/after record state for mutations), and `context` carries request metadata (client IP). Scoped reads filter the envelopes directly (`target` for per-record history, `actor` for per-actor history); a dedicated audit junction (permissioned read surface with denormalized sort keys) is a later slice. The access layer only inserts and selects; events are immutable. What writes it today: the login flow (account self-creation, logins), logout, and the `/users` area's mutations. **Generic resource mutations do not write audit entries yet** — that wiring is a deferred slice (§11); the `AuditEvent`/`AuditAction` contract and the table are already in place.

## 7. The HTTP layer (`twentytoo`)

### 7.1 State and registry

`AppState` is everything handlers share: the `ResourceRegistry` (one erased meta per registered resource — key, label, policy probes, cheap count), the `TemplateEngine`, the `FlagService`, the `default_actor`, and the optional `AuthService`. The registry feeds the home dashboard (cards filtered by `can_view_any`, with counts from one cheap `Page.total` call each) and the nav (resource entries plus the gated Users entry).

### 7.2 The builder: declarative surface and boot validation

`Twentytoo::builder()` is the whole declarative surface: `.resource(...)` registers resources, `.with_template_dir(...)` sets user template overrides, `.default_actor(...)` / `.db(...)` / `.auth(...)` / `.code_sender(...)` configure identity. `build()` then **fails at boot** when:

- any resource's declared identifiers (fields, columns, search fields, filters, sorts) fail `adapter.validate()` — the safety net for JSON/API adapters without compile-time checks;
- every referenced template name doesn't resolve after the environment build;
- auth is configured without a database, or a workspace resource claims the `users` key.

The built instance hands out `into_router()` (nest inside a larger axum app) or `into_make_service()` (ready for `axum::serve`).

### 7.3 Route table and generic handlers

Per resource, monomorphized sub-routers carry `ResourceState<R>` (shared app state + one concrete resource):

- `GET/POST /resources/{key}` — list view (search, filters, sort, pagination) and create;
- `GET /resources/{key}/new` — create form;
- `GET/POST /resources/{key}/{id}` — detail view and update;
- `GET /resources/{key}/{id}/edit` — edit form;
- `POST /resources/{key}/{id}/delete` — delete.

Plus `/` (dashboard home) with a fallback, the auth routes, and the users area when configured. Handlers are generic over `Resource`, one implementation for every resource — no per-resource handler boilerplate. Views are shaped in Rust (`ResourceView`, `FieldView`, `PagerView`, …) with field visibility (`visible_to`), editability (`editable_by`), and policy gates applied per actor **before anything reaches a template**. Form posts arrive via a custom multi-value `FormData` extractor (repeated keys → `Vec<String>`), are validated into entity JSON with field-level errors re-rendered on 422, and round-trip payload → entity → JSON so entity validators run. Server-managed entity fields (e.g. `created_at` set by the DB) need `#[serde(default)]` on typed entities.

### 7.4 Middleware

One request-pipeline middleware (`actor_layer`) runs for every route: without auth it injects the configured default actor; with auth it resolves the session cookie to the expanded actor — requests without a valid session are denied (`302 /login` for GET/HEAD, `401` otherwise) — except the public auth routes (`/login`, `/login/*`, `/logout`), which run without an actor. On any database failure during resolution, it denies.

### 7.5 Errors

Hand-rolled error enums with `Display` + `source()`; no `thiserror`/`anyhow` anywhere. `DataError` (§5.3) covers the adapter contract; `AppError` maps policy denials to `Forbidden`, adapter failures through to their HTTP equivalents, and template failures to logged 500s; `BuildError` reports boot-time misconfiguration. `DbError` does the same for the database layer.

### 7.6 Flags

`FlagService` is an in-memory on/off registry: **a flag that was never set is off** — the conservative reading. `Resource::flag()` gates a resource: a disabled flag renders the resource as **404, not 403** — don't leak the existence of a flagged resource. Targeting strategies (role/user/percentage rollouts) and a runtime flag-management UI are deferred (§11).

## 8. Templates (MiniJinja)

### 8.1 The MiniJinja decision

The template engine is **Jinja2 — concretely MiniJinja 2.x** (a native Rust implementation of the Jinja2 language, maintained by Jinja2's author; `COMPATIBILITY.md` is the deviation ledger). Tera and Askama were considered and rejected:

- The killer requirement is **dynamic field rendering** — templates iterate a runtime `Vec<Field>` — which excludes compile-time engines (Askama).
- Against Tera, Jinja2 wins on being the industry's most widely known server-side template language (Django/Flask/Ansible/dbt/Airflow), the entire Jinja2 editor toolchain (`.j2` files are recognized everywhere), the maintenance pedigree, and the satellite ecosystem: `minijinja-embed` (templates compiled into the binary, **build-time syntax validation**), `minijinja-autoreload` (dev hot reload), `minijinja-cli` (shell rendering).
- MiniJinja functions receive a `State` — they can read the render context (the actor) without it being threaded through every call site.

Templates are a consumer-facing surface: teams override built-ins and write custom pages, so the language is chosen for the people who will read it.

### 8.2 Naming and inventory

HTML templates end in `.html.j2` (`layout/base`, `dashboard/home`, `resource/list`, `resource/detail`, `resource/form`, `partials/pagination`, `auth/email`, `auth/code`, `auth/password`, `users/list`, `users/form`); plain-text formats (email bodies, exports) would use other extensions. The `BUILTIN_TEMPLATES` list is the boot-check inventory (§8.5).

### 8.3 Autoescape: a framework rule, not a default

MiniJinja does not autoescape by default. The framework sets it explicitly: **any template that emits HTML (`.html.j2`) is autoescaped; anything else renders raw.** Safe-string-returning functions escape internally — every dynamic fragment escaped, structure framework-owned — and the rest of the template is autoescaped by the environment. One reviewer-facing rule: safe-string functions escape internally; templates escape by default.

### 8.4 Functions and filters

Registered once at environment build:

| Kind | Name | Purpose |
| --- | --- | --- |
| function | `can(permission)` | RBAC check; reads the actor from render `State` — no actor argument, so it can't be forgotten or faked; no actor in context → deny |
| function | `format_field(value, kind)` | Render one cell/detail value per `FieldKind` (badge pill, relation link, …) |
| function | `format_filter(filter)` | The sidebar control for one filter |
| function | `form_control(field, values)` | One form widget, current values kept on error re-renders |
| filter | `format_datetime(fmt)` | chrono-backed date rendering; accepts RFC 3339 strings |
| filter | `currency` | Money formatting for `Currency` fields |

`flag`/`metric_value` register with their respective future slices.

### 8.5 Embedding, overrides, and the boot check

The environment is built once at startup in three steps: (1) built-ins are compiled into the binary via `build.rs` (`minijinja_embed::embed_templates!`) — **invalid syntax fails the build, not the first request**; (2) user templates from the override directory are registered by name and **replace** built-ins; (3) a path loader catches templates that exist only in the user's directory. The env is `Send + Sync`, built once, shared as `Arc` — the same env renders every handler and any custom page. A boot check (`get_template` on every `BUILTIN_TEMPLATES` name) catches missing or mistyped references before the first request; CI renders every built-in against fixture data.

### 8.6 SSR-first rendering with Tabler

The client stack is the vendored Tabler 1.4.0 bundle: `tabler.min.css` + `tabler.min.js` (Bootstrap components and Tabler behaviors, served from the binary like every other asset) plus a tiny `app.js` enhancement script. There is no htmx and no partial-swap protocol: every navigation is a full page load. List controls are plain HTTP — the toolbar is a GET form (search + filters), sort headers and pagination are links, and all of that state lives in the URL query string. Mutations are POST forms that 303 to their destination carrying a `?flash=<kind>:<message>` param; the layout renders it as a Tabler toast on the landed page (the `Flash` extractor in `presentation/extractors.rs` parses it, and `app.js` hands the toast to the Tabler API for autohide). Modal confirmations (delete) and menus use Tabler's `data-bs-*` attributes and require the JS bundle; forms and links never do.

The framework owns its static assets: `web/static/` (the vendored Tabler bundles plus `app.js`) is embedded by `build.rs` into a name → bytes table and served from the binary at `/static/{*path}` — the handler never touches the filesystem. `StaticFiles` (infrastructure) does the lookup and maps extensions to content types; unknown names answer 404. `BUILTIN_ASSETS` lists every asset the built-in templates reference, and the boot check verifies each one is embedded. Nothing in the framework depends on a CDN — users can re-vendor a newer Tabler or swap the shell wholesale.

The design language and UI kit — the Tabler class contract, component specs, interaction patterns — live in `01-ui-kit.md`; §8's templates and assets render against that contract.

## 9. The database layer (`twentytoo-db`)

PostgreSQL via sqlx 0.8, owning the framework's schema only:

- **Migrations** (`0001_users` … `0006_login_tokens`), embedded via `MIGRATOR` and applied by `Db::migrate()`: users, groups + group membership, sessions, roles/permissions + grant tables, the `inapp_events` audit stream, login tokens.
- **A typed access layer** on the `Db` pool handle: `queries/users`, `queries/groups`, `queries/sessions`, `queries/login_tokens`, `queries/access` (permissions, roles, grants, and `load_actor`), `queries/audit` — with row shapes in `entities/`.
- Queries are **runtime-bound** (`sqlx::query_as`), so the crate compiles and unit-tests with no live database; integration tests run against `DATABASE_URL` and skip when it's unset.
- `DbError` mirrors the hand-rolled error convention.

The generic per-resource `SqlxAdapter` for user entities is deliberately out of scope here — this crate covers only the framework-owned tables (§11).

## 10. The demo

`examples/demo` is the end-to-end proof: two resources (`Users`, `Stores`) on `InMemoryAdapter` with seeded data, behind the real login flow. With auth enabled, unauthenticated requests redirect to `/login`, sessions live in PostgreSQL, and the framework's `/users` area manages the accounts. It needs the compose Postgres (or a `DATABASE_URL`); sign in with the bootstrap admin `admin@example.com` / `admin1234`.

## 11. Deferred slices

Named in the code as arriving later; the contracts are already in place where noted:

- **Generic per-resource `SqlxAdapter`** — user entities against arbitrary tables (shared-store shape); `twentytoo-db` currently covers only framework-owned tables.
- **Audit wiring for generic resource mutations** — create/update/delete currently write no audit entries (§6.5).
- **Actions** — contract types exist; no HTTP surface renders or executes them.
- **Metrics** — `Resource::metrics()` deliberately absent; the `Aggregation` contract (§5.8) is ready.
- **Module system** — the builder registers resources directly; no `Module` trait yet.
- **Flag targeting strategies** — `FlagService` is a plain on/off map; no role/user/percentage targeting, no runtime management UI.
- **Relationship tabs** — `Resource::relationships()` is declared but not rendered.
- **File/image uploads** — kinds exist; form fields exclude them for now.
- **Streaming exports (CSV/Excel)** — `stream()` default exists; no export UI.
- **Schema-discovery auto-configuration** — `describe()` exists; no point-at-a-table flow.
- **Adapter decorators** (caching, retry, rate limit, read-only, enrichment) — none shipped.
- **Impersonation**; **roles & permissions management UI** (grants are database-level today).

## 12. Dropped and rejected

Concepts from earlier brainstorms that were never built and are no longer part of the design intent:

- **SSE broadcasting / live updates, notifications, saved filters, scheduled tasks, API keys, per-user dashboard customization, record comments, import wizard (CSV/Excel mapping), dark mode, i18n, global search** — all considered; none shipped, none required by anything that exists. (Theming landed in `01-ui-kit.md` on Tabler's `--tblr-*` variables; dark mode itself stays dropped.)
- **Custom `Page` primitive** — the escape hatch is plain axum (`into_router()` + custom handlers over the same state/templates); no dedicated page API.
- **Proc-macro DSL and derive macros** — rejected; the builder + `field!`/`fields!` `macro_rules!` macros keep standard Rust tooling working and the surface iteration-free.
- **Workflow/approval chains, drag-and-drop builders, public-facing portals, native mobile apps, GraphQL APIs** — non-goals.
- **Tera, Askama** — rejected as template engines (§8.1). **diesel, sea-orm, sea-query, actix-web, yew/leptos, jsonwebtoken, thiserror, anyhow** — rejected or unused dependencies.
- **CSV export of list views** — not shipped; exports arrive with the streaming slice.

## 13. Decision ledger

| Decision | Choice | Why |
| --- | --- | --- |
| Reference language | Rust (MSRV 1.94) | Misconfiguration caught at compile time; traits map naturally to the spec; zero-cost policy/template paths; single static binary |
| HTTP stack | axum 0.8 + tokio | Tower middleware model matches the cross-cutting concerns; maintained by the tokio team |
| Template engine | MiniJinja 2.x | §8.1 |
| Declarative surface | Builder + `macro_rules!` field macros | Pure Rust: rust-analyzer, fmt, clippy all work unchanged; no DSL grammar to maintain |
| Registry storage | Trait objects (`Box<dyn …>` / `Arc<dyn …>`) | Heterogeneous resources from anywhere; dispatch cost is noise next to the DB round-trip |
| Defaults | Deny / off / unsupported | An unconfigured system is safe, never permissive |
| Validation timing | Fail at boot | `build()` validates identifiers, templates, and auth preconditions before serving |
| View-layer data | Entities as serialized JSON only | The engine treats typed and `serde_json::Value` entities identically |
| Errors | Hand-rolled enums, `Display` + `source()` | No thiserror/anyhow anywhere |
| Client-side JS | Vendored Tabler bundle (Bootstrap components) + one tiny optional `app.js` | SSR-first; every navigation is plain HTTP; Tabler components (modals, dropdowns, toasts) ride on top; the JS policy lives in `01-ui-kit.md` §9 |
