# Repository Guidelines

## Project Overview

Twentytoo is an internal-tools dashboard framework: teams declare resources, fields, actions, metrics, pages, and policies instead of writing CRUD code. The design intent lives in two consolidated records: `brainstorms/00-architecture.md` (system design — Rust is the confirmed reference language, MiniJinja for templating) and `brainstorms/01-ui-kit.md` (design language & UI kit: light-only `--tt-*` tokens, component specs, htmx interaction patterns, the vanilla-JS policy).

The workspace ships the **core contract** (traits + the `InMemoryAdapter` reference implementation), the **HTTP layer** (generic CRUD handlers over axum, a MiniJinja template engine with framework functions, the built-in `.j2` templates, and a builder that assembles the router with boot-time validation), and the **database layer** (PostgreSQL migrations + a typed access layer for the framework-owned tables: users, sessions, groups, roles, permissions, and the append-only audit log). Audit logging, the generic SQLx resource adapter, actions, metrics, and the module system are the deferred slices ("arrive in later slices" — `crates/twentytoo/src/lib.rs`).

## Architecture & Data Flow

Three-crate Cargo workspace (`Cargo.toml`):

- **`crates/twentytoo-core`** — the library every other slice builds on. 13 modules, one concept each. Runtime-agnostic: no tokio, no HTTP, no IO.
- **`crates/twentytoo`** — the HTTP layer: axum handlers, MiniJinja templates, and the app builder; re-exports `twentytoo_core::*` and mirrors its `prelude`.
- **`crates/twentytoo-db`** — PostgreSQL (sqlx 0.8): the embedded schema (`migrations/`, `MIGRATOR`) and a typed access layer on a `Db` pool handle — users/sessions (auth), groups/`group_members` (groupings), roles/permissions with `user_roles` + `group_roles` and `load_actor` expanding grants into the core `Actor` (RBAC), and the append-only audit log. Runtime-bound queries, so the crate builds with no live database; integration tests in `tests/db.rs` run against `DATABASE_URL` and skip when it is unset (CI provides a Postgres service).

Central contract — `DataAdapter<E, Id = String>` (`crates/twentytoo-core/src/adapter.rs`), an `#[async_trait]` with a **graded** surface:

- Required: `capabilities()`, `list(&Query) -> Page<E>`, `get(&Id)`.
- Defaulted: `get_many`, `create`/`update`/`delete`, `apply_mutations`, `begin() -> TxAdapter`, `aggregate`, `stream`, `describe`, `validate`. Defaults are **conservative** — unsupported operations return `DataError::Unsupported`, not panics or optimistic stubs.

Data flow:

1. A `Resource` impl (`resource.rs`) declares `Entity`, `fields()`, and `adapter() -> Arc<dyn DataAdapter<Self::Entity>>`.
2. The engine builds a `Query` — the policy scope is already merged into the query filter; **adapters never see policies**.
3. Writes arrive as `Mutation<Id>` (Create/Update/Delete/Upsert) wrapped in `WriteContext` (expected_version, idempotency_key, actor).
4. `capabilities()` is read once at boot; the UI degrades to the declared capability grade.

`InMemoryAdapter<E>` (`in_memory.rs`) is a complete HashMap-backed engine: filter tree, offset + base64 cursor pagination, multi-column sort with nulls ordering, search, projection, transactions (`InMemoryTx`), aggregations, streaming. It is the reference implementation that proves the contract.

HTTP layer (`crates/twentytoo/src/`): `container.rs` (composition root: `TwentytooBuilder` → axum router with boot validation), `application/` (`dto.rs` view models, `payload.rs` form → entity JSON with field-level validation, `query.rs`, `auth/` login flow), `infrastructure/` (`templates.rs` — MiniJinja env: autoescape by extension, `can`/`format_field`/`format_filter`/`form_control` functions, `format_datetime`/`currency` filters, built-ins embedded via `build.rs` with user-override dir + path loader; `static_files.rs` — the embedded-asset file service behind `/static`; `flags.rs`), `presentation/` (`handlers/` — per-resource monomorphized routers carrying `ResourceState<R>`: `mod.rs` owns the route table + shared extractors, `list.rs`/`detail.rs`/`forms.rs`/`mutations.rs` one handler concern each, `home.rs` dashboard + fallback, `auth.rs`/`users.rs`, `static_files.rs` the `/static` handler, `helpers.rs` private internals; `middleware.rs` request pipeline, `extractors.rs`, `registry.rs`, `state.rs`), `shared/` (`errors.rs` `AppError`/`BuildError`, `utils.rs`). Built-in templates live in `web/templates/` (`.j2`), static assets in `web/static/` (css + vendored htmx) — both embedded at build time. The demo (`examples/demo/`) boots two resources on `InMemoryAdapter` behind the real login flow (`00` §10).

## Key Directories

| Path | Purpose |
|---|---|
| `crates/twentytoo-core/src/` | The contract: `adapter.rs` (DataAdapter/TxAdapter), `resource.rs`, `field.rs` (+ `field!`/`fields!` macros), `query.rs`, `write.rs`, `actor.rs`, `policy.rs`, `action.rs`, `capabilities.rs`, `aggregation.rs`, `audit.rs`, `error.rs`, `in_memory.rs` |
| `crates/twentytoo/src/` | The HTTP layer: `container.rs`, `application/` (dto, payload, query, auth), `infrastructure/` (templates, static_files, flags), `presentation/` (handlers, middleware, extractors, registry, state), `shared/` (errors, utils) |
| `web/templates/` | Built-in `.j2` templates, embedded by `build.rs` (build-time syntax check) |
| `web/static/` | Built-in static assets (css, vendored htmx), embedded and served at `/static` by the static-file service |
| `crates/twentytoo-db/` | The DB layer: `migrations/` (users, groups, sessions, roles, permissions, group assignments, audit log), `entities/` (row shapes: user, group, session, permission, role, audit), `queries/` (typed access: `users`, `groups`, `sessions`, `access` with `load_actor`, `audit`), `db.rs` (`Db` pool + `MIGRATOR`), `error.rs` (`DbError`), `tests/db.rs` (live-Postgres integration) |
| `crates/twentytoo/examples/demo/` | Demo app: members + stores on `InMemoryAdapter`, behind the real login flow (needs the compose Postgres; admin@example.com / admin1234) |
| `brainstorms/` | The consolidated design records (`00-architecture.md` system design; `01-ui-kit.md` design language & UI kit); source of truth for intent and decisions |
| `.github/workflows/ci.yml` | The only quality gate config |

## Development Commands

Plain cargo, from the repo root — no Makefile, scripts, or toolchain pin:

```bash
cargo build                 # whole workspace
cargo test --workspace      # unit + doctests; twentytoo-db DB tests skip without DATABASE_URL
cargo clippy --workspace --all-targets -- -D warnings   # CI gate: warnings are errors
cargo fmt --all --check     # CI gate: stock rustfmt defaults, no rustfmt.toml
```

`twentytoo-db` integration tests need a PostgreSQL; the target database is created on first use:

```bash
DATABASE_URL=postgres://localhost/twentytoo_test cargo test -p twentytoo-db
```

MSRV is `rust-version = 1.94` (edition 2024, resolver 3) — older toolchains hard-fail. CI runs current stable only; it does not verify MSRV. `Cargo.lock` is committed.

## Code Conventions & Common Patterns

- **Error handling**: hand-rolled error enums with `Display` + `source()` — `DataError` (`error.rs`: NotFound, Conflict, Validation, Unauthorized, RateLimited, Unsupported, Internal). No thiserror/anyhow anywhere.
- **Async**: `#[async_trait]` for all async traits; no tokio/async-std in the dependency tree. `parking_lot::RwLock` for sync state.
- **Trait objects over generics where possible**: `Arc<dyn DataAdapter<E>>`, `Box<dyn TxAdapter>`, `&dyn Policy`. `Id` is a defaulted generic on `DataAdapter` (not an associated type) to keep `Arc<dyn DataAdapter<E>>` object-safe.
- **Conservative defaults**: `Policy` methods deny by default (`DenyAll` baseline); `DataAdapter` defaults return `Unsupported`; `Capabilities::default()` is a read-only baseline and every upgrade is explicit.
- **Entities are data**: they travel as `serde_json::Value`; domain types carry no serde derives and use `PhantomData` as the `E` anchor (see `Field<E>` in `field.rs`, which hand-rolls `PartialEq` to stay `E`-bound-free).
- **Declarative field DSL**: `field!`/`fields!` macros (`field.rs`) — `field!("status", "Status", Badge { options: &[("open", "Open")] }, list: true)`. The crate doc in `crates/twentytoo/src/lib.rs` shows canonical usage.
- **Exports**: `#![warn(missing_docs)]` — every public item needs doc comments. Flat re-exports of all public types from `lib.rs` plus a `prelude` module that also re-exports the macros. `twentytoo` re-exports `twentytoo_core::*` flat too.
- **Explicit returns**: `[workspace.lints.clippy]` denies `clippy::implicit_return` — every non-`()` function/closure must end in `return expr;` (tail expressions are rejected; `clippy::needless_return` is disabled to match). Exception: `#[async_trait]` items carry `#[allow(clippy::implicit_return)]` — the macro rewrites the body tail to `Box::pin(...)`, so the lint can only see the wrapper and its auto-fix corrupts the item (see `adapter.rs`, `action.rs`, `in_memory.rs`, `registry.rs`, `app.rs`).
- **Template layer** (`00` §8): built-ins (from `web/templates/`) are embedded with build-time syntax validation (`build.rs` → `minijinja_embed::embed_templates!`); autoescape is set explicitly (`.html.j2` → Html); safe-string-returning functions (`format_field`, `format_filter`, `form_control`) escape internally; minijinja 2.23+ has no `Environment::render`/serde `downcast` — use `get_template(name)?.render(ctx)` and `ViaDeserialize<T>`/serde extraction.
- **Static assets** (`00` §8.6): assets in `web/static/` embed via `build.rs` (generated `include_bytes!` table); `StaticFiles` (`infrastructure/static_files.rs`) is lookup + MIME-by-extension; the `/static/{*path}` handler serves from the binary — never the filesystem; new assets referenced by built-in templates must be added to `StaticFiles::BUILTIN_ASSETS` (boot check).
- **axum 0.8 gotchas**: path captures are `{id}`, not `:id`; `Router::nest` requires identical state types (per-resource routers are state-baked to `Router<()>` and nested via `nest_service`); `axum::serve` only accepts `Router<()>` or an `IntoMakeService` (hence `Twentytoo::into_make_service`); middleware layer state must be the extractable type (`State<AppState>` extracts via `FromRef`); `axum::Extension` still exists; form bodies are parsed by the custom multi-value `FormData` extractor (repeated keys → `Vec<String>`), not `Form<HashMap<String, Vec<String>>>` which rejects single values.
- **Server-managed entity fields** (e.g. `created_at` set by the DB) need `#[serde(default)]` on typed entities — form payloads only carry `show_in_form` fields and the handler round-trips payload → `E` → JSON to run entity validators.
- **Naming**: snake_case; trait methods short and behavior-descriptive; test names `<operation>_<scenario>[_<outcome>]` (e.g. `insert_duplicate_conflicts`, `update_missing_not_found`).
