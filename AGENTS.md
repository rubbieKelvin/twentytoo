# Repository Guidelines

## Project Overview

Twentytoo is an internal-tools dashboard framework: teams declare resources, fields, actions, metrics, pages, and policies instead of writing CRUD code. The design intent lives in `brainstorms/00-init.md` through `brainstorms/05-template-engine.md` (numbered decision records; 05 is the latest — Rust is the confirmed reference language, MiniJinja for templating). The workspace currently ships only the **Phase-1 core contract**: the trait/type surface plus an in-memory reference implementation. Handlers, templates, and the module system are explicitly deferred ("arrive in later slices" — `crates/twentytoo/src/lib.rs`).

## Architecture & Data Flow

Two-crate Cargo workspace (`Cargo.toml`):

- **`crates/twentytoo-core`** — the library every other slice builds on. 13 modules, one concept each. Runtime-agnostic: no tokio, no HTTP, no IO.
- **`crates/twentytoo`** — thin facade: re-exports `twentytoo_core::*` and mirrors its `prelude`.

Central contract — `DataAdapter<E, Id = String>` (`crates/twentytoo-core/src/adapter.rs`), an `#[async_trait]` with a **graded** surface:

- Required: `capabilities()`, `list(&Query) -> Page<E>`, `get(&Id)`.
- Defaulted: `get_many`, `create`/`update`/`delete`, `apply_mutations`, `begin() -> TxAdapter`, `aggregate`, `stream`, `describe`, `validate`. Defaults are **conservative** — unsupported operations return `DataError::Unsupported`, not panics or optimistic stubs.

Data flow:

1. A `Resource` impl (`resource.rs`) declares `Entity`, `fields()`, and `adapter() -> Arc<dyn DataAdapter<Self::Entity>>`.
2. The engine builds a `Query` — the policy scope is already merged into the query filter; **adapters never see policies**.
3. Writes arrive as `Mutation<Id>` (Create/Update/Delete/Upsert) wrapped in `WriteContext` (expected_version, idempotency_key, actor).
4. `capabilities()` is read once at boot; the UI degrades to the declared capability grade.

`InMemoryAdapter<E>` (`in_memory.rs`) is a complete HashMap-backed engine: filter tree, offset + base64 cursor pagination, multi-column sort with nulls ordering, search, projection, transactions (`InMemoryTx`), aggregations, streaming. It is the reference implementation that proves the contract.

## Key Directories

| Path | Purpose |
|---|---|
| `crates/twentytoo-core/src/` | The contract: `adapter.rs` (DataAdapter/TxAdapter), `resource.rs`, `field.rs` (+ `field!`/`fields!` macros), `query.rs`, `write.rs`, `actor.rs`, `policy.rs`, `action.rs`, `capabilities.rs`, `aggregation.rs`, `audit.rs`, `error.rs`, `in_memory.rs` |
| `crates/twentytoo/src/` | Facade crate (re-exports only, for now) |
| `brainstorms/` | Design docs 00–05; source of truth for intent and decisions |
| `.github/workflows/ci.yml` | The only quality gate config |

## Development Commands

Plain cargo, from the repo root — no Makefile, scripts, or toolchain pin:

```bash
cargo build                 # whole workspace
cargo test --workspace      # unit + doctests
cargo clippy --workspace --all-targets -- -D warnings   # CI gate: warnings are errors
cargo fmt --all --check     # CI gate: stock rustfmt defaults, no rustfmt.toml
```

MSRV is `rust-version = 1.94` (edition 2024, resolver 3) — older toolchains hard-fail. CI runs current stable only; it does not verify MSRV. `Cargo.lock` is committed.

## Code Conventions & Common Patterns

- **Error handling**: hand-rolled error enums with `Display` + `source()` — `DataError` (`error.rs`: NotFound, Conflict, Validation, Unauthorized, RateLimited, Unsupported, Internal). No thiserror/anyhow anywhere.
- **Async**: `#[async_trait]` for all async traits; no tokio/async-std in the dependency tree. `parking_lot::RwLock` for sync state.
- **Trait objects over generics where possible**: `Arc<dyn DataAdapter<E>>`, `Box<dyn TxAdapter>`, `&dyn Policy`. `Id` is a defaulted generic on `DataAdapter` (not an associated type) to keep `Arc<dyn DataAdapter<E>>` object-safe.
- **Conservative defaults**: `Policy` methods deny by default (`DenyAll` baseline); `DataAdapter` defaults return `Unsupported`; `Capabilities::default()` is a read-only baseline and every upgrade is explicit.
- **Entities are data**: they travel as `serde_json::Value`; domain types carry no serde derives and use `PhantomData` as the `E` anchor (see `Field<E>` in `field.rs`, which hand-rolls `PartialEq` to stay `E`-bound-free).
- **Declarative field DSL**: `field!`/`fields!` macros (`field.rs`) — `field!("status", "Status", Badge { options: &[("open", "Open")] }, list: true)`. The crate doc in `crates/twentytoo/src/lib.rs` shows canonical usage.
- **Exports**: `#![warn(missing_docs)]` — every public item needs doc comments. Flat re-exports of all public types from `lib.rs` plus a `prelude` module that also re-exports the macros.
- **Naming**: snake_case; trait methods short and behavior-descriptive; test names `<operation>_<scenario>[_<outcome>]` (e.g. `insert_duplicate_conflicts`, `update_missing_not_found`).
