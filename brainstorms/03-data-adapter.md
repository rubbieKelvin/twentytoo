# 03-data-adapter.md — The DataAdapter, evolved

**Status:** Pre-implementation design
**Depends on:** [00-init.md](./00-init.md) (core spec), [01-rust-implementation.md](./01-rust-implementation.md) (§2.3, §5)
**Date:** 2026-08-11

---

## 0. Why this doc

The v1 `DataAdapter` in 01-rust-implementation.md §2.3 is a good SQL adapter interface. It is not yet a good *data source* interface. This doc reworks it so the framework can sit in front of as many kinds of data sources as possible — SQL databases, document stores, search engines, OLAP/warehouses, HTTP APIs, GraphQL endpoints, flat files, in-memory stores — without the source contorting itself to the engine's assumptions, and without the engine pretending every source can do everything.

The rule of thumb throughout: **the engine adapts to the source's capabilities; the source never fakes the engine's expectations.**

## 1. The source taxonomy

What "as many cases as possible" concretely means — the shapes of data sources an internal-tools framework will realistically be pointed at:

| Source kind | Examples | Native query model | Typical write support | Pagination |
| ----------- | -------- | ------------------ | --------------------- | ---------- |
| Relational SQL | Postgres, MySQL, SQLite, SQL Server | SQL WHERE/ORDER BY | Full CRUD + transactions | Offset or keyset |
| Document store | MongoDB, DynamoDB | JSON filters, query DSL | Full CRUD, often composite keys | Cursor (DynamoDB `ExclusiveStartKey`, Mongo `_id`) |
| Search engine | Elasticsearch/OpenSearch | Query DSL, relevance ranking | Read-mostly (index rebuilds) | `from/size` or `search_after`, no reliable total |
| OLAP / warehouse | ClickHouse, DuckDB, BigQuery | SQL, aggregation-heavy | Append-only | Offset, cheap counts |
| HTTP API | Stripe, GitHub, internal REST | URL query params | Partial (POST/DELETE per convention) | Link headers / cursor params, often no total |
| GraphQL | internal services | GraphQL query language | Mutations, often restricted | `first/after` cursors (Relay) |
| gRPC / protobuf | internal services | Method-specific messages | Per-service | Per-service |
| Flat files | CSV/JSON on disk or S3 | In-memory scan | None | Trivial |
| In-memory | test doubles, demo data | Hash maps | Full | Trivial |

The axes of variation that matter to the framework:

- **Query expressiveness** — which filter operators, sorts, and searches the source can natively express.
- **Pagination** — offset vs. cursor; whether a filtered total is cheap (page numbers) or impossible (load-more).
- **Write support** — none, CRUD, bulk; transactions; idempotency; optimistic concurrency.
- **Schema** — fixed & typed (SQL) vs. discoverable (ES mappings, `information_schema`) vs. opaque (API).
- **Consistency** — read-after-write guarantees, eventual consistency for search indexes.
- **Cost profile** — count queries, N+1 round trips, rate limits, per-request latency.

Every one of these axes has a place in the design below. None of them is handled by the v1 trait.

## 2. Where the v1 trait breaks

Concrete failure modes, each with the source that triggers it:

| # | v1 assumption | Breaks on | Failure |
| - | ------------- | --------- | ------- |
| 1 | `list(page, per_page)` + `PaginatedResult.total: u64` | Stripe, GitHub, Elasticsearch | No page numbers available; total is expensive (ES) or absent (APIs). Framework renders pagination that doesn't exist. |
| 2 | `create`/`update`/`delete` in the trait | Search engines, third-party read-only APIs | Implementor must write error-returning stubs. No way for the framework to know the resource is read-only and hide the buttons. |
| 3 | Flat `&[Filter]` (AND only) | Any source with nested query semantics (ES `bool`, GraphQL `OR`) | The engine can't express the query the source *can* answer. Framework silently ships a weaker UI than the source supports. |
| 4 | Scalar `Id` | DynamoDB (partition+sort key), APIs with non-string ids | Trait says `Id: Display` — fine — but the framework's URL/identity model assumes one string; composite keys need an explicit story. |
| 5 | `actor: &Actor` in write signatures | Every adapter | Audit/policy is the engine's job (AuditLayer, policy hooks). Baking the actor in couples the data layer to the session model and makes one adapter instance unshareable across resources/policies. |
| 6 | `aggregate(...) -> serde_json::Value` | Trend/partition/table metrics from the core spec | Untyped results; no group-by, no date-histogram, no typed buckets. Every metric adapter invents its own JSON shape. |
| 7 | No batch reads | List views with relation columns | 25 rows × 1 query per relation = N+1. The framework can't batch because the trait can't fetch many ids at once. |
| 8 | `list` returns all columns | Wide tables, expensive API payloads | No projection; the adapter fetches everything even when the view shows three columns. |
| 9 | No streaming | CSV/Excel export of large filtered views (02 §9) | Export must page through `list` (25k rows × 40 requests) or load everything into memory. |
| 10 | One adapter instance per entity | Any deployment with >2 resources | The `SqlxAdapter<E> { table }` shape re-creates a pool/config per entity. One database should mean one store, not N adapters. |
| 11 | No capability signal | Every source | The engine cannot adapt its UI (hide sort, swap pagination, drop filters) because the adapter can't say what it supports. |
| 12 | No startup schema check | JSON-entity adapters, API adapters | A typo in a `list_columns` entry fails at first page load, not at boot. Violates the project's "misconfiguration caught before deploy" promise. |

## 3. Design principles

1. **The view layer touches entities only as serialized JSON.** Already true in the Tera layer (`item[col]`), it becomes a rule: typed entities are an *adapter-side* optimization, never a framework requirement. This unlocks `E = serde_json::Value` for dynamic sources (see §11) without touching the engine.
2. **The adapter is actor-agnostic.** Policy scoping becomes a filter the engine merges into the query; audit stays in the middleware; actions/hooks receive the actor from the engine. One adapter instance can serve many resources and many policies (§10).
3. **The query model is the contract.** A bounded `Query` struct — filter tree, multi-sort, search spec, pagination, projection — that every source can translate to its native form. Not a full query language; a bounded set of operations with universal meaning (§4).
4. **Capabilities, not stubs.** Adapters declare what they support; the engine consults the declaration at startup to build a per-resource UI feature matrix, and at runtime to choose behavior. Unsupported operations return `DataError::Unsupported` only as a defensive backstop (§6).
5. **Defaults over errors.** Every advanced method (`get_many`, `stream`, `apply_mutations`, `validate`) has a sane default implementation. Read-only adapters implement ~3 methods, not 9.
6. **Composition over hierarchy.** Cross-cutting concerns (caching, retry, rate limiting, read-only enforcement, multi-source enrichment) are decorators wrapping an inner adapter, not new trait methods and not fork points (§12).
7. **Fail at boot, not at first click.** A startup validation pass checks every declared field/sort/filter against the source (§11.3). This is the adapter's half of the framework's build-time-safety promise.

## 4. The query model

One struct family, translatable by every source.

### 4.1 Filters — a tree, not a list

```rust
/// A filter operator with source-independent meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterOp {
    Eq, Ne,
    Gt, Gte, Lt, Lte,
    In, NotIn,
    Contains,       // substring containment
    StartsWith,
    IsNull, IsNotNull,
    FullText,       // relevance-ish match; adapter maps to tsvector / multi_match / q
}

#[derive(Clone, Debug)]
pub enum FilterValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    DateTime(DateTime<Utc>),
    In(Vec<serde_json::Value>),
    Range { gt: Option<serde_json::Value>, gte: Option<serde_json::Value>,
            lt: Option<serde_json::Value>, lte: Option<serde_json::Value> },
}

/// A filter tree: AND/OR/NOT over field predicates.
#[derive(Clone, Debug)]
pub enum FilterNode {
    Field { field: String, op: FilterOp, value: FilterValue },
    And(Vec<FilterNode>),
    Or(Vec<FilterNode>),
    Not(Box<FilterNode>),
}
```

The tree is the minimum that makes ES `bool` queries, GraphQL `OR`, and SQL `WHERE (a OR b) AND c` all expressible. The framework's list-view params build a tree (filter sidebar → `And`; date ranges → `Range`); the adapter flattens or nests as its source requires. A source that can't express `Or` declares so in capabilities (§6) and the engine simply doesn't offer OR-combining filter UIs for it.

### 4.2 Sort, search, pagination, projection

```rust
pub struct SortField { pub field: String, pub dir: SortDir, pub nulls: NullsOrder }
pub enum SortDir { Asc, Desc }
pub enum NullsOrder { First, Last, Default }

/// What "search" means for this source; the engine renders the matching UX.
pub enum SearchMode { None, Exact, Substring, FullText }

pub struct SearchSpec { pub term: String, pub fields: Vec<String> }

pub enum Pagination {
    /// page is 1-based; the classic numbered-pager contract.
    Offset { page: usize, per_page: usize },
    /// Opaque cursor; adapter-encoded, framework-blind.
    Cursor { after: Option<String>, before: Option<String>, per_page: usize },
}

pub struct Query {
    pub pagination: Pagination,
    pub sort: Vec<SortField>,           // multi-column
    pub filter: Option<FilterNode>,     // user filters ∧ policy scope, already merged
    pub search: Option<SearchSpec>,
    pub projection: Option<Vec<String>>, // None = all fields
}
```

Notes:

- **Sort is a list** — the first SQL adapter only shipped single-column sort, but multi-column sort is free to support in the model and some sources (ES, warehouses) need it.
- **`SearchSpec.fields`** comes from the resource's `search_fields`; the adapter validates them at boot.
- **Policy scope is merged into `filter` by the engine**, before the adapter sees the query. The adapter never knows about teams, roles, or row scoping — that's the engine's policy layer doing its job. `WHERE owner_id = $1` is just another `Eq` node.
- **`Query` is `Clone + Debug`** — cheap to build per request, loggable, and reusable by the streaming default impl (§9).

### 4.3 The page result

```rust
/// Opaque to the framework; the adapter encodes its own continuation state.
pub struct Cursor(pub String);   // e.g. base64 of (last_id, sort_state) or an API's cursor

pub struct Page<E> {
    pub items: Vec<E>,
    /// None when the source can't count cheaply (APIs, ES). The UI switches
    /// to prev/next navigation when total is absent.
    pub total: Option<u64>,
    pub next: Option<Cursor>,
    pub prev: Option<Cursor>,
    /// Echo of the request pagination; tells the UI which render mode to use.
    pub pagination: Pagination,
}
```

The framework renders exactly one of two pagers:

- `total: Some(n)` → numbered pages (offset mode) — the familiar `« 1 2 3 … 17 »`.
- `total: None` → prev/next (or "load more" via htmx), driven purely by cursors.

An adapter may *receive* `Offset` and translate internally to keyset (SQL adapters do this when the source can't do cheap offset), or receive `Cursor` and return `total: None`. The engine doesn't care — it only reads the result.

## 5. Writes

### 5.1 The write context

```rust
pub struct WriteContext<'a> {
    /// Optimistic concurrency: adapter compares and fails with Conflict.
    pub expected_version: Option<Version>,
    /// HTTP adapters: Idempotency-Key header. SQL adapters: an idempotency
    /// column / unique constraint. Import retries use this.
    pub idempotency_key: Option<&'a str>,
    /// Escape hatch for the rare source that authenticates per-user.
    /// Most adapters ignore it.
    pub actor: Option<&'a Actor>,
}

pub struct Version(pub String);  // opaque: DB row version / API etag
```

`expected_version` gives the "two agents processed the same record" case (02 §2) a real answer: the second writer gets `DataError::Conflict` and the UI shows "this record changed — reload," instead of a silent last-write-wins. This is cheap for SQL adapters (a version column or `updated_at` comparison) and standard for APIs (`If-Match`).

### 5.2 Mutation set

```rust
pub enum Mutation<Id> {
    Create { data: serde_json::Value },
    Update { id: Id, patch: serde_json::Value },
    Delete { id: Id },
    Upsert { id: Id, data: serde_json::Value },
}
```

The import wizard (02 §9) is the driving case: validate → map → dry-run → `apply_mutations` in one call, inside a transaction when the source supports one (§7), row-by-row with error skipping when it doesn't. `Mutation::Upsert` exists because CSV re-imports are "create or update by id" — the most common real import shape.

## 6. Capabilities — the engine adapts

```rust
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub pagination: PaginationModes,      // Offset | Cursor | Both
    pub totals: bool,                     // cheap filtered counts (page numbers)
    pub write: WriteCapability,           // ReadOnly | Crud | Bulk
    pub transactions: bool,
    pub search: SearchMode,               // None | Exact | Substring | FullText
    pub filter_ops: Vec<FilterOp>,        // operators the source can express
    pub sort: bool,                       // any sort at all
    pub aggregation: AggregationCapability, // None | Basic | Grouped | Histogram
    pub concurrency: ConcurrencySupport,  // None | Version | Etag
    pub streaming: bool,                  // efficient native streaming (vs. default paging)
    pub schema_discovery: bool,           // can introspect its own schema
}
```

**The payoff is the UI adaptation table.** At startup the resource engine builds a per-resource feature matrix from `adapter.capabilities()`, and every generated view consults it:

| Capability | Engine behavior when absent |
| ---------- | --------------------------- |
| `write = ReadOnly` | No create/edit/delete buttons, no bulk actions, no form routes. The resource degrades to a browse/export surface. |
| `totals = false` | Prev/next (or htmx "load more") pagination instead of numbered pages. |
| `search = None` | Search box not rendered. |
| `search = FullText` | Search box rendered with relevance hint; adapter may add a relevance sort option. |
| `filter_ops` | Only filter controls the source can express are offered. No `Contains` filter on a source that only supports `Eq`. |
| `sort = false` | Sortable headers not rendered; `default_sort` ignored (or applied adapter-side). |
| `aggregation = None` | Metric cards for this resource not offered; the dashboard simply shows other metrics. |
| `transactions = false` | Import wizard falls back to row-by-row with per-row error reporting. |
| `concurrency = None` | No stale-edit detection; UI doesn't promise it. |

Capabilities are read once at boot and cached in the per-resource feature matrix — no per-request overhead, and the values are static per adapter anyway (a capability is a property of the source, not of the request).

`DataError::Unsupported` remains in the error enum as a defensive backstop for engine bugs, not as the primary signaling mechanism.

## 7. Transactions

```rust
#[async_trait]
pub trait TxAdapter<E>: Send + Sync {
    async fn get(&self, id: &Id) -> Result<Option<E>, DataError>;
    async fn apply(&mut self, mutations: &[Mutation<Id>]) -> Result<(), DataError>;
    async fn commit(self: Box<Self>) -> Result<(), DataError>;
    async fn rollback(self: Box<Self>) -> Result<(), DataError>;
}

// on DataAdapter:
async fn begin(&self) -> Result<Box<dyn TxAdapter<E>>, DataError>;  // default: Unsupported
```

- SQL adapters: a real transaction, all mutations atomic.
- In-memory adapters: a `RwLock`-guarded batch.
- API adapters: `Unsupported`; the engine uses the sequential fallback.
- `TxAdapter` is a separate sub-trait so the main trait stays object-safe and read-only adapters never see transaction machinery.

The import wizard flow: `capabilities.transactions?` → `begin` → `apply_mutations` (all rows) → `commit`; on any error → `rollback` + per-row report. Without transactions: `apply_mutations` sequentially with error skipping, exactly the fallback the core spec's import section describes.

## 8. Aggregation — typed, metric-shaped

The core spec's five metric types dictate the shape:

| Metric type | Aggregation |
| ----------- | ----------- |
| `value` | `measure` only (Count / Sum / …) |
| `trend` | `group_by: DateHistogram` |
| `partition` | `group_by: Field` |
| `table` | `group_by: Field` + sort + limit |
| `progress` | two aggregate calls (numerator / denominator), composed by the engine |

```rust
pub struct Aggregation {
    pub measure: Measure,
    pub group_by: Option<GroupBy>,
    pub filter: Option<FilterNode>,
    pub sort: Vec<SortField>,
    pub limit: Option<usize>,
}

pub enum Measure {
    Count,
    Sum(String), Avg(String), Min(String), Max(String),
    Distinct(String),
}

pub enum GroupBy {
    Field(String),
    DateHistogram { field: String, interval: Interval },
}

pub enum Interval { Minute, Hour, Day, Week, Month, Quarter, Year }

pub struct AggregationResult {
    pub value: Option<f64>,          // set when no group_by
    pub buckets: Vec<Bucket>,        // set when group_by
}

pub struct Bucket { pub key: serde_json::Value, pub value: f64 }
```

Adapter mapping is mechanical: SQL → `COUNT/SUM/… GROUP BY date_trunc(…)`; ES → `aggs: { terms / date_histogram }`; ClickHouse → native. An adapter with `aggregation = None` simply doesn't back metrics — a `StripeAdapter` powers CRUD views but the dashboard's metric cards come from the warehouse adapter instead. That's the read-model story: **metrics attach to an adapter, not to a resource's primary source** (see §12.3).

`count()` from the v1 trait is dropped — `aggregate(Measure::Count, filter)` covers it, and pagination totals are now the adapter's internal concern (`Page.total`).

## 9. Batch reads and streaming

```rust
/// Default: sequential get(). Override for real batching (IN (...), batch API).
async fn get_many(&self, ids: &[Self::Id]) -> Result<Vec<E>, DataError>;

/// Default: page through list(). Override for native cursors / server-side streams.
async fn stream(&self, query: Query) -> BoxStream<'static, Result<E, DataError>>;
```

- **`get_many`** kills the N+1 on list views with relation columns: when a page of 25 stores renders 25 "latest order" cells, the engine collects the foreign keys, calls the related resource's adapter `get_many` once, and joins in the view model. Contract: returned order is not guaranteed; the engine keys by id.
- **`stream`** is the export path (02 §9, CSV/Excel of the *filtered* view): one call, a `Stream` of rows, written to the response as it arrives. The default implementation pages through `list` and is fine up to tens of thousands of rows; SQL adapters override with a keyset cursor stream for million-row exports. The engine never materializes the full result set in memory.
- `stream` takes `Query` by value (not borrow) so the stream is `'static` and the adapter can't outlive-borrow itself — adapters clone what they need, or `Arc` their inner store.

## 10. The shared-store pattern

The v1 `SqlxAdapter<E> { pool, table }` re-creates a pool per entity. The v2 shape separates the store from the adapter:

```rust
/// One per database. Owns the pool and the discovered schema.
pub struct SqlxStore {
    pool: PgPool,
    schema: RwLock<HashMap<&'static str, TableSchema>>,  // discovered at boot
}

/// One per resource; cheap, shareable, policy-agnostic.
pub struct SqlxAdapter<E> {
    store: Arc<SqlxStore>,
    table: &'static str,
    _marker: PhantomData<E>,
}
```

Consequences:

- One `SqlxStore` serves every resource on a database. Adding a resource is one struct + one registration, not one pool.
- Two resources can share one table with different policies — the engine's policy-merged filter handles scoping, so nothing about the adapter changes.
- Typed entities (`E = Store`, `FromRow`) still work, but the same store also serves `E = serde_json::Value` (below), so a team can point the framework at an existing table and get a working resource with zero struct definitions.

## 11. Dynamic sources: JSON entities, schema discovery, boot validation

### 11.1 `E = serde_json::Value`

`serde_json::Value` satisfies `Serialize + DeserializeOwned` — the trait bounds. An adapter over `Value` maps rows/hits/documents to JSON objects by field name, and the view layer reads `item["name"]` exactly as it reads `item.name` on typed entities. The engine treats both identically; typed entities are purely an adapter-side type-safety choice.

This is the single biggest "as many cases as possible" unlock: **DynamoDB, Elasticsearch, a Stripe-like API, a CSV file, and a legacy Postgres table all become resources with the same engine, and a team can stand up a resource for any of them without writing a Rust struct or a field list.**

### 11.2 Schema discovery

```rust
pub struct FieldSpec { pub name: String, pub kind: FieldKind, pub nullable: bool }

/// Default: Err(Unsupported). Sources with discoverable schemas return
/// their columns / mappings / a sample document.
async fn describe(&self) -> Result<Vec<FieldSpec>, DataError>;
```

Adapters with `schema_discovery: true` can back the "auto-configure" path: the engine offers a `fields()` implementation derived from `describe()` (SQL column types → `FieldKind`: `text → Text`, `numeric → Number`, `timestamp → DateTime`, `bool → Boolean`, JSONB → `Json`, …). `FieldKind` inference is a best-effort starting point, overridable per field in the resource definition. The resource definition remains the source of truth; discovery just fills in the defaults.

### 11.3 Boot validation — fail at startup

```rust
/// Default: Ok(()). Checks that every declared field/sort/search/filter
/// identifier exists in the source.
async fn validate(&self, identifiers: &[&str]) -> Result<(), DataError>;
```

At build time the resource engine collects every identifier the resource declares (`fields()`, `list_columns`, `search_fields`, filter fields, `default_sort` field, relationship keys) and calls `adapter.validate()` once per resource. A typo in `list_columns` or a filter on a nonexistent column fails the boot with a precise error — *before* the first page load. For typed SQLx adapters this is redundant with compile-time checks (harmless); for JSON/API adapters it is the safety net that keeps the framework's "misconfiguration caught before deploy" promise. API adapters without introspection can implement `validate` as `Ok` and accept the loss.

## 12. Composition

### 12.1 Decorators

Every trait method takes `&self`, so wrapping is trivial. Decorators are the standard answer to cross-cutting source concerns:

```rust
pub struct CachingAdapter<E>     { inner: Box<dyn DataAdapter<E>>, cache: Cache }   // get/get_many/list TTL
pub struct RetryAdapter<E>       { inner: Box<dyn DataAdapter<E>>, policy: RetryPolicy } // transient failures, rate limits
pub struct RateLimitAdapter<E>   { inner: Box<dyn DataAdapter<E>>, limiter: Limiter }
pub struct ReadOnlyAdapter<E>    { inner: Box<dyn DataAdapter<E>> }  // capability write = ReadOnly; write methods → Unsupported
```

Composition example — an HTTP adapter with all the production concerns, built in `Module::init`:

```rust
let adapter = RateLimitAdapter::new(
    RetryAdapter::new(
        CachingAdapter::new(
            StripeAdapter::new(client),
            cache,
        ),
        retry_policy,
    ),
    limiter,
);
```

`ReadOnlyAdapter` is worth shipping: it converts any CRUD source into a read-only resource declaratively (an ops dashboard over a production DB, a "no accidental writes" guard for a shared integration), and it's the honest implementation of the "buttons don't render" rule — the capability change, not a policy, drives the UI.

### 12.2 Enrichment — one resource, two sources

The "orders in Postgres, fulfillment status in a third-party API" case. The engine should not know about it; composition handles it:

```rust
/// After each read, batch-enrich records from a second source.
pub struct EnrichingAdapter<E, F> {
    inner: Box<dyn DataAdapter<E>>,
    enrich: F,   // Fn(&mut [E], &EnrichCtx) -> Result<(), DataError>, called with the whole page
}
```

The enrichment closure receives the full page (or stream chunk) and can batch against a secondary source — the `get_many`-style batching discipline applies to the *enricher* too. Because enrichment is a decorator, the engine sees one adapter and one resource; the two-source nature is invisible to policies, audit, and the view layer. If enrichment gets slow, a `CachingAdapter` around the whole thing fixes it. If the composite is genuinely a different beast (state machines, multi-step flows), the answer remains the custom `Page` escape hatch — the decorator covers the common "read from A, decorate from B" shape.

### 12.3 Read models and warehouses

Metrics and heavy reporting queries often belong on a different source than CRUD (ClickHouse/warehouse for aggregates, Postgres for rows). Two patterns, both already supported:

1. **A metric declares its own adapter.** `Metric` gains `adapter()` (default: the resource's adapter). A `pending_doctor_approvals` metric resolves against the warehouse adapter while the `doctors` resource CRUDs against Postgres. The engine runs `aggregate` on whichever adapter the metric names.
2. **Read-model resources.** A team materializes a denormalized view (Postgres view, ES index, ClickHouse table) and points a resource at it — possibly `ReadOnlyAdapter`-wrapped. The dashboard becomes a browse surface over the read model with zero write risk.

Both are plain adapter configuration, not new framework machinery.

## 13. The v2 trait

```rust
#[async_trait]
pub trait DataAdapter<E>: Send + Sync
where
    E: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    type Id: Clone + Send + Sync + Serialize + DeserializeOwned
           + std::fmt::Display + std::str::FromStr;

    // ---- Identity ----
    fn capabilities(&self) -> Capabilities;

    // ---- Reads (must implement) ----
    async fn list(&self, query: &Query) -> Result<Page<E>, DataError>;
    async fn get(&self, id: &Self::Id) -> Result<Option<E>, DataError>;

    // ---- Batch read (default: sequential get) ----
    async fn get_many(&self, ids: &[Self::Id]) -> Result<Vec<E>, DataError>;

    // ---- Writes (default: Unsupported) ----
    async fn create(&self, data: serde_json::Value, ctx: &WriteContext<'_>)
        -> Result<E, DataError>;
    async fn update(&self, id: &Self::Id, patch: serde_json::Value, ctx: &WriteContext<'_>)
        -> Result<E, DataError>;
    async fn delete(&self, id: &Self::Id, ctx: &WriteContext<'_>)
        -> Result<(), DataError>;
    async fn apply_mutations(&self, mutations: &[Mutation<Self::Id>], ctx: &WriteContext<'_>)
        -> Result<(), DataError>;   // default: sequential create/update/delete

    // ---- Transactions (default: Unsupported) ----
    async fn begin(&self) -> Result<Box<dyn TxAdapter<E>>, DataError>;

    // ---- Aggregation (default: Unsupported) ----
    async fn aggregate(&self, agg: &Aggregation) -> Result<AggregationResult, DataError>;

    // ---- Streaming (default: page through list) ----
    async fn stream(&self, query: Query) -> BoxStream<'static, Result<E, DataError>>;

    // ---- Introspection (defaults: Ok / Unsupported) ----
    async fn describe(&self) -> Result<Vec<FieldSpec>, DataError>;
    async fn validate(&self, identifiers: &[&str]) -> Result<(), DataError>;
}
```

```rust
pub enum DataError {
    NotFound,
    Conflict,                // optimistic concurrency / unique violation
    Validation(String),
    Unauthorized,            // the *source's* credentials failed
    RateLimited,
    Unsupported,             // capability violation (defensive backstop)
    Internal(Box<dyn std::error::Error + Send + Sync>),
}
```

Read-only adapters implement: `capabilities`, `list`, `get` — optionally `get_many`, `stream`, `describe`. Everything else has a default. The "one adapter to rule them all" trap is avoided: this is a *graded* contract, and the grade is declared, not discovered by trial.

### 13.1 The contract test double

An `InMemoryAdapter<E>` (HashMap-backed, full capabilities: writes, transactions, aggregation, streaming) ships with the framework. It serves three purposes: the demo app (no Postgres required), the test suite, and — most importantly — it is the **reference implementation of the trait contract**. Every default and every capability is exercised against it in CI; a new adapter implementor gets a working example that proves the trait's behavior is implementable as specified.

## 14. Framework integration changes

### 14.1 List handler flow (updated from 01 §4.2)

```
ListParams (page / cursor, sort, filters, search, columns)
        │
        ▼
Resource feature matrix (from capabilities, computed at boot)
        │   ── pagination mode, search mode, sortable, filter ops
        ▼
Build Query:  pagination ← params
              filter     ← policy_scope(actor) ∧ user_filters
              search     ← search_fields × term
              projection ← visible columns
        │
        ▼
adapter.list(&query) ──► Page<E> { items, total?, next?, prev? }
        │
        ▼
Pager render:  total? → numbered pages     |     else → prev/next (htmx load-more)
```

The handler no longer branches on adapter type — it reads `Page` and the feature matrix. The same handler drives a Postgres resource with numbered pages, a Stripe resource with load-more, and a read-only ES resource with no create button.

### 14.2 Resource trait

`Resource` gains `fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>>` (constructed in `Module::init`, where pools and clients are built). `ModuleContext` already exists for exactly this. The v1 doc's `SqlxAdapter<E> { pool, table }` per-entity construction disappears; modules build `SqlxStore` once and hand out `SqlxAdapter` wrappers.

## 15. Migration path

The v2 trait supersedes 01 §2.3 and §5. The deltas, all engine-internal except the adapter implementations:

| v1 | v2 | Consumer impact |
| -- | -- | --------------- |
| `list(page, per_page, sort, filters, search)` | `list(&Query)` | Handler + adapter |
| `PaginatedResult{total: u64}` | `Page{total: Option, next, prev}` | Handler + pager template |
| `create/update/delete(..., actor)` | `(..., &WriteContext)` | Adapter; actor captured by engine |
| `count(filters)` | `aggregate(Count, filter)` | Metrics; list totals internal to adapter |
| `aggregate -> Value` | `aggregate -> AggregationResult` | Metric implementations |
| — | `capabilities()` | Engine boot (feature matrix) |
| — | `get_many`, `stream`, `apply_mutations`, `begin`, `describe`, `validate` | Engine features: N+1 batching, exports, imports, schema discovery, boot validation |

Phasing:

- **Phase 1:** Query model, capabilities, `Page`, `WriteContext`, `InMemoryAdapter` + Postgres adapter (typed and JSON modes), boot validation, feature matrix. The Postgres adapter exercises every capability; the demo runs on `InMemoryAdapter` so a checkout with no database still boots.
- **Phase 2/3:** `stream` (exports), `apply_mutations` + `begin` (import wizard), `get_many` (relation-column batching), decorators (`ReadOnlyAdapter`, `CachingAdapter`, `RetryAdapter`), first HTTP API adapter as the worked example (Stripe-shaped: cursor pagination, no totals, idempotency keys, etags).
- **Phase 4:** `describe`-driven auto-config, warehouse adapters for metrics.

## 16. Open questions (for the next brainstorm)

1. **Per-user source auth.** `WriteContext.actor` and startup-built adapters cover most cases, but a source that authenticates per-user (a third-party API with per-user tokens) needs request-scoped adapters. Ship a `RequestAdapterFactory` seam now, or defer until a real case exists? Leaning: defer.
2. **Raw passthrough.** Should `Query` gain a `Raw(serde_json::Value)` variant for adapter-native queries (advanced custom-page filters), or does that undermine the "bounded model every source translates" principle? Leaning: no raw variant in v1; custom pages can hold a concrete adapter type and call its own methods directly — the escape hatch already exists.
3. **Multi-source joins.** Enrichment decorators cover "read from A, decorate from B," but a resource whose *list* needs a real join across two sources (A × B) is out of scope. Custom page, or a `JoinAdapter` later? Leaning: custom page; joins are where adapter abstractions die.
4. **`describe` + auto-config scope.** Do we ship the auto-configure flow (point at a table → working resource) in Phase 1, or only `validate`? Auto-config is a demo-worthy feature but touches the builder; leaning: `validate` in Phase 1, auto-config Phase 4.
5. **Search mode granularity.** `SearchMode` is one mode per adapter. A Postgres adapter could honestly do `Substring` (ILIKE) *and* `FullText` (tsvector) depending on the field. Is single-mode-per-adapter too coarse? Leaning: keep single mode; the adapter picks its best; per-field mode is a Phase-4 refinement.
6. **Bulk semantics.** `WriteCapability::Bulk` signals efficient `apply_mutations`. Should it also cover idempotent retry of partially-applied batches (the "import ran twice" problem)? The `idempotency_key` on `WriteContext` is the per-call answer; a batch-level key may be needed. Leaning: per-call first, batch-level when an import-retry case exists.
7. **`Cursor` encoding.** Opaque base64 strings — framework-blind by design. One risk: cursors that outlive schema changes (a keyset cursor referencing a dropped column). Adapters should version their cursor payloads (`v1:...`). Agreed, or is there a case for framework-readable cursors?

## 17. What this is not

- **Not a query-language abstraction.** The `Query` model is a bounded set of operations with universal meaning — deliberately not SQL-for-everything. Sources with richer semantics keep them adapter-side.
- **Not a universal ORM.** No cross-source joins, no implicit relation graph in the engine, no automatic migration generation for non-SQL sources.
- **Not write-through caching.** Caching decorators are opt-in and TTL-based; no coherence protocol, no invalidation registry. Live updates stay the broadcasting feature's job (02 §2).
- **Not change-data-capture.** A CDC-fed read-model adapter is just another adapter (and a good one), but the framework doesn't provide CDC plumbing in v1.
- **Not a promise that every source gets every feature.** The whole point of §6 is the reverse: the framework visibly and honestly degrades to what the source supports, and the UI never lies about it.
