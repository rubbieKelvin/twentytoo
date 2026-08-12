# 04-framework-language.md — Reference Stack: Rust, Python, Elixir, TypeScript

**Status:** Brainstorm — decision pending
**Depends on:** [00-init.md](./00-init.md) (core spec), [01-rust-implementation.md](./01-rust-implementation.md) (Rust design), [02-extra-features.md](./02-extra-features.md) (features), [03-data-adapter.md](./03-data-adapter.md) (adapter spec)
**Date:** 2026-08-12

---

## 0. The question

[00-init.md](./00-init.md) deliberately keeps the core spec stack-agnostic, and [01](./01-rust-implementation.md) picked Rust as the reference implementation with a full design. This doc re-opens that choice before significant code exists: given the spec as written, which language and framework should the reference implementation actually be built in?

Framing matters here. The spec is the product; the reference implementation is the proof. But the reference stack is not a neutral detail — it determines:

- **The consumer experience.** Resource, action, and policy definitions are written in the reference language by the teams adopting the framework. That language *is* the framework's API surface.
- **How much of the "boring" list is already built.** Auth, sessions, CSRF, RBAC, migrations, validation — the spec says "use battle-tested libraries; don't innovate on security plumbing." The stack decides how much of that list is pre-solved.
- **Iteration speed on the framework itself.** This is a large framework: six primitives, a generic CRUD engine, a query-translation layer (03), templates, four built-in modules. Months of work. Language productivity is not a rounding error here.

The comparison criteria below are taken from the spec itself, not from generic language advocacy.

---

## 1. What the spec actually demands of a stack

| # | Requirement | Source | Why it discriminates |
| - | ----------- | ------ | -------------------- |
| 1 | Declarative resource/field/action/policy definitions, close to the DSL examples in §4 | 00 §2, §4 | Some languages express declarative configs far more naturally than others |
| 2 | Misconfiguration caught before deploy | 00 §2 ("compile-time safety pays off"), 03 §11.3 ("fail at boot, not at first click") | Rust gets this at compile time; others must build startup checks + CI |
| 3 | SSR-first, HTML over the wire, progressive enhancement, **no JS build step required** | 00 §1, §2 | Rules out every React-based shape; favors htmx-style or LiveView-style stacks |
| 4 | Multi-source data adapters: SQL, search engines, HTTP APIs, warehouses, flat files | 03 §1 taxonomy | The framework must ship adapters or lean on the ecosystem for source clients |
| 5 | Boring security plumbing: auth, sessions, CSRF, XSS-safe templates, password hashing | 00 §2 | Python/Django has this consolidated; other stacks assemble it |
| 6 | RBAC: role checks + row-level policies, enforced at four points | 00 §5 | Row-level policy is framework work in *every* stack; role-level differs |
| 7 | Audit log, append-only, per-record and global views | 00 §5.5 | Middleware/decoration friendliness of the stack |
| 8 | Real-time: SSE broadcast, presence, notifications (P2+) | 02 §2, §3 | LiveView+PubSub makes this nearly free; elsewhere it's a project |
| 9 | Background jobs: async actions, CSV export completion | 02 §9 | Queue ecosystem maturity |
| 10 | Distributed as a library, consumed by composition, not fork | 00 §1 | Packaging story of each language |
| 11 | Performance | 01 §0 | Real but overrated for internal tools: tens of concurrent operators, latency-bound by DB queries. Rust's margin rarely binds. Honest downgrade from 01's framing. |

---

## 2. Rust / axum — the current design

[01](./01-rust-implementation.md) is a strong, complete design. The case for it, restated:

**What's genuinely good**
- The compile-time story is real. A typo'd permission string, a field referenced in `list_columns` but not declared, a wrong adapter method — these are build errors, not support tickets. This is the strongest form of the spec's "caught before deploy" promise, and it doubles as framework quality control: the generic CRUD engine is heavily generic code, and the compiler keeps it honest.
- Traits map cleanly onto the six primitives. `Resource`, `DataAdapter`, `Policy`, `Action` as traits is a clean surface (01 §2).
- Performance and operational profile: single static binary, low latency, no GC pauses. Deployment is trivial — genuinely pleasant for an ops-facing tool.
- Tower middleware composes auth/session/audit/flag layers exactly the way the spec's cross-cutting concerns want.

**What it costs — stated honestly**
- **The DSL is the weakest of the four.** The spec's own examples (`resource "stores" { ... }`) are config-language shaped. Rust renders them as structs + builder methods + trait impls — readable, but the noisiest consumer API of the four candidates. 01 admits this ("the DSL would be less verbose") and defers macros to "a future optimization." For a framework whose whole thesis is "stand up tools fast," the consumer-facing definition language is the product. This is the core tension.
- **Iteration speed on the framework itself.** The generic engine (per-entity typing, adapter trait bounds, serde round-trips, async trait objects) is the hardest part of Rust. Every new field kind, filter operator, or renderer touches generic code. Expect roughly 2–3x the calendar time of a dynamic-language implementation for the same feature set. 01's own mitigations (value-typed `Field`, `E = serde_json::Value` escape hatch) help but don't change the curve.
- **Consumer talent pool.** The people who write resource definitions at adopting teams are often not Rust engineers — support tooling is built by domain engineers. Requiring Rust fluency to add a resource, an action handler, or a custom page is the single biggest adoption barrier of any candidate.
- **Multi-source ecosystem is thinnest of the four.** sqlx/sea-orm are excellent for SQL; but Stripe, GitHub, Elasticsearch, ClickHouse, and the long tail of internal HTTP APIs each become adapter-integration work. Clients exist; fewer are first-party, and every new source is real engineering, not a pip install.
- **Templates.** Tera is serviceable (01 §12 shows the pattern works) but the dev loop — hot reload, debugging, rich component libraries — is the weakest of the four.

**Verdict:** the right choice *if* compile-time guarantees are the brand and the building team is Rust-fluent. The wrong choice if consumer adoption speed and definition-language ergonomics are the priorities — and the spec's §1 and §2 read like they are.

---

## 3. Python / Django

The problem domain — permissioned CRUD consoles over arbitrary data — is Django's native habitat. Django admin is the oldest and most widely deployed "internal tools framework" in existence; Twentytoo is the same class of product with a better declarative model.

**The stack:** Django (sync views + Jinja templates + htmx), the framework as a library layered on top. FastAPI is wrong here — it's an API framework with no admin/auth/session/forms story; the framework would rebuild Django's boring list from scratch.

**What's free (the 00 §2 "boring where it counts" list, pre-built and 20 years battle-tested)**
- Auth: users, passwords (Argon2 default since Django 4.x), sessions, login flows, password reset. The entire §6 user-management module starts from this instead of building it.
- CSRF, XSS-safe auto-escaping templates, SQL injection protection via the ORM, clickjacking headers. Not a checklist to assemble — defaults.
- Forms + validation, including server-side validation of every field kind in §4.2 (Django form fields cover text/email/date/datetime/number/boolean/select/multiselect/file/image natively).
- Migrations (schema + data) — the framework's own tables (users, roles, permissions, flags, audit) get migrations for free.
- Admin-grade per-model permissions (add/change/delete/view) as the role-level layer of §5.
- System checks framework (`django.core.checks`) — a first-class mechanism for 03 §11.3 "fail at boot": field/sort/filter/column validation runs in `manage.py check` and at startup, in CI and before deploy. This recovers most of the Rust compile-time promise in a dynamic language.
- The biggest consumer talent pool and the deepest multi-source client ecosystem of any candidate: Stripe, GitHub, Elasticsearch, ClickHouse, warehouses, `requests`/`httpx` for the API long tail, pandas for CSV/Excel exports.

**The declarative model:** Python's declarative classes are the most natural expression of the spec's DSL. `class Store(Resource): name = fields.Text(required=True, list=True)` is one line from the spec's `resource "stores"` block. Metaclasses/`__init_subclass__` give the "definition IS the source of truth" property with zero generated code. Resource definitions are plain Python — the lowest-friction consumer API of the four.

**What's still framework work (honest — Django does not hand you the product)**
- **Row-level policies (§5.3).** Django's permission model is per-model, not per-record. `StorePolicy.can_view(actor, record)` is framework code, exactly as it is in every stack.
- The action/metric/page primitives, the resource engine, list/detail/form rendering, filter/sort/pagination — all framework code. Django gives conventions and utilities (class-based views, admin patterns as reference), not the product.
- **Append-only immutable audit** — Django's own admin log is mutable; the framework rolls its own table (simple_history/auditlog are close but don't enforce immutability). One table + middleware, not a research project.
- The **UI shell**: the spec wants its own look (sidebar groups, badges, tables) — a set of Django templates + htmx, not django-admin's chrome.
- The 03 adapter layer: Django's ORM is SQL-first; non-SQL sources are adapters that don't use the ORM. Fine, but it means the framework doesn't lean on the ORM's query machinery for those sources — same as every other stack.
- Real-time (02 §2): Django Channels or SSE via `StreamingHttpResponse`. Workable, but this is the stack's weakest spot versus Elixir — async is supported, not native.

**Risks**
- Type safety is the weakest of the four at the definition surface; the startup-check + CI discipline must actually be built and enforced (it's a check registry, not a compiler).
- Django's weight: framework-as-library on Django means consumers carry Django's conventions (settings, apps, migrations). Acceptable — it's the price of the boring list.
- The 00 §1 "interfaces should port" discipline is on the team, not the stack; Django's idioms are seductive and will leak into the spec if unchecked. Same discipline 01 already demands of Rust.

**Verdict:** the strongest overall fit for the spec's stated values — declarative DSL, boring security, stand-up-fast, biggest consumer pool. The framework does the interesting 40% (policies, actions, metrics, adapters, UI); Django's boring 60% is already done.

---

## 4. Elixir / Phoenix (LiveView)

If there is a stack that was *designed* for the 00 §1 rendering model — "SSR, HTML over the wire, optional enhancement, no build step" — it's Phoenix LiveView: server-rendered DOM, diffed over a websocket, zero client framework, no JS build. LiveView exceeds what the spec asked for: htmx gives you partial swaps on demand; LiveView gives you them by default, with real-time for free.

**The stack:** Phoenix + LiveView + Ecto, Oban for jobs, PubSub/Presence for 02 §2–3.

**What's free**
- **The entire 02 §2 broadcasting section.** Channels, record-level subscriptions, presence ("User X is viewing this record"), toast notifications — Phoenix PubSub + Presence + LiveView are built-in primitives. In every other candidate this is a bespoke SSE/WS project.
- **Background jobs:** Oban is the standard, first-class OTP supervision. Async exports, notification delivery — structured concurrency that doesn't fight you.
- **The declarative model:** Elixir macros produce the most natural DSL of the four candidates. The spec's `resource "stores" { ... }` blocks are *exactly* what Elixir DSLs look like. `use Twentytoo.Resource` + `field :name, :text, required: true` is idiomatic Elixir.
- **Ecto** for SQL: excellent migrations, query composition that maps cleanly onto 03's filter-tree model.
- **Fault tolerance:** supervision trees mean a crashed action handler or exporter doesn't take the dashboard down. For an ops tool, "boring where it counts" includes "keeps running."
- Boot-time validation culture (`mix` compile-time warnings, Ecto schema validation, LiveView's compile-time route checking) gives a decent misconfiguration story; the framework adds a boot check pass for 03 §11.3.

**The Ash question — must be faced head-on**
Ash Framework (AshPostgres + AshAdmin + AshJsonApi) already implements a remarkable share of this spec: declarative resources, actions (including custom actions with input validation), attribute-based policies with relationship-aware rules, multitenancy, notifiers (pub/sub), and AshAdmin — an admin UI. Choosing Elixir means choosing a relationship to Ash: **build Twentytoo as a layer on top of Ash** (faster, but the six primitives and policy model become Ash's, and the 03 adapter taxonomy fights Ash's Postgres-centric data layer), or **compete with it** (Twentytoo must be demonstrably better than AshAdmin to justify existing). This is a strategic question that doesn't exist in the other three stacks.

**Risks**
- **Thinnest multi-source ecosystem.** Stripe/GitHub/search/warehouse clients exist but are fewer, less maintained, and more DIY than Python's or TS's. 03's taxonomy (§1) is the most work in Elixir.
- **Talent pool is the smallest of the four**, and the consumer bar is real: resource definitions require Elixir, and custom pages require LiveView, which is a genuinely new mental model for most engineers.
- LiveView is the render model — the "interfaces should port" goal (00 §1) gets harder because the rendering layer is inseparable from Phoenix. htmx stacks port; LiveView doesn't.
- BEAM ops: releases are fine, but memory profile and runtime tooling are unfamiliar territory for teams coming from anything else.

**Verdict:** the most *complete* realization of the spec's vision — the framework would be genuinely delightful and real-time by default. It's the right call when real-time collaboration and OTP robustness are priorities, the building team is (or wants to become) an Elixir team, and the Ash relationship is resolved. It's the riskiest call on ecosystem and adoption.

---

## 5. TypeScript / Node

TypeScript is the pragmatic middle: type safety approaching Rust's at the definition surface, ecosystem depth approaching Python's, and — for TS-everywhere orgs — the consumer language is the language the whole company already speaks.

**Which framework shape?** The spec's rendering model (no JS build step, HTML over the wire) rules out the two most popular options:

- **Next.js / Remix: wrong shape.** React SSR still requires a client bundle, hydration, and a build step for any interactivity — exactly what 00 §1 forbids ("no SPA build step is required to use the framework"). Remix's forms-first philosophy is spiritually close, but you'd be fighting React's runtime for the whole life of the project. Skip both.
- **AdonisJS** — the closest Django analog in Node: first-party session auth, CSRF, validation, migrations, Lucid ORM, Edge templates. Framework-as-library on Adonis is the coherent path: the boring list is mostly pre-assembled.
- **Hono + htmx + BYO** — the closest analog to the Rust path: lightweight, excellent middleware, but auth/sessions/CSRF/RBAC are assembled by the framework team, and the "boring where it counts" promise is the weakest of any candidate.

**What's genuinely good**
- **Type safety at the definition surface.** Discriminated unions on `FieldKind`, generics on `Resource<T>`, `satisfies` checks on field configs — most of the "typo'd permission string" class of bugs becomes compile errors, with far less machinery than Rust. The best dynamic-language safety story of the four.
- **Deepest ecosystem after Python** for multi-source adapters: stripe-node, octokit, Elasticsearch, ClickHouse clients, Drizzle/Kysely for SQL.
- **One language for the whole org.** If Twentytoo's consumers are internal teams that already write TypeScript, the adoption argument is decisive — resource definitions need zero new learning.
- Fast iteration, best-in-class dev experience (Vite), strong testing culture. npm distribution of a library is the easiest of the four.

**What's still framework work**
- **Security plumbing is the youngest of the four.** Node's auth/session/CSRF story is fragmented and churning (Lucia deprecated → better-auth, and it's not Django-grade yet). The framework team owns more of the "boring" list here than in any other candidate — including the parts the spec explicitly says not to innovate on.
- **SSR template culture.** No first-class Jinja/Tera/HEEx equivalent; server-rendered HTML + htmx in TS means the framework invents its own template conventions (JSX-on-server, Eta, or similar) with less ecosystem support and weaker XSS-by-default guarantees than Django's auto-escaping.
- Background jobs are external (BullMQ/Redis) — fine, same as Python/Rust.
- Framework churn risk: the Node ecosystem's half-life is short; a 2-year framework build has a real chance its foundation libraries shift underneath it.

**Verdict:** the adoption-maximizing choice for TypeScript-first orgs, with genuinely good type safety. The cost is owning security plumbing that Django ships, and building SSR conventions that don't exist in the ecosystem. Right call when the consumer org is TS-everywhere; otherwise the framework does more undifferentiated work for the same product.

---

## 6. Comparison matrix

Scored against §1's requirements (A+ best … C weakest). These are directional, not precise.

| Criterion | Rust / axum | Python / Django | Elixir / Phoenix | TS / AdonisJS |
| --------- | ----------- | --------------- | ----------------- | ------------- |
| Declarative DSL ergonomics | C+ | A- | A | B+ |
| Misconfig before deploy | A+ (compile) | B+ (system checks + CI) | B+ (boot checks) | A- (compile) |
| SSR + htmx, no build step | A | A | A+ (LiveView) | B (conventions DIY) |
| Boring security plumbing | B+ | A | B | B- |
| Multi-source adapter ecosystem | C+ | A | C | A- |
| Real-time / broadcast | B | B (Channels/SSE) | A+ (PubSub/Presence) | B+ (SSE/WS) |
| Background jobs | C+ | B+ (Celery/Django-Q) | A (Oban/OTP) | B (BullMQ) |
| Framework + consumer iteration speed | C | A | B+ | B+ |
| Consumer talent pool | C+ | A | C | A- |
| Runtime ops profile | A+ (single binary) | B | B | B+ |
| Library distribution | A | A | A | A+ |
| Strategic wildcard | — | — | Ash Framework overlap | Framework churn |

---

## 7. The deciding questions

The matrix doesn't decide; these four questions do. They're org-specific, and the answers change the recommendation:

1. **Who builds it, and who writes resource definitions first?** If the building team is Rust-fluent and the first consumers are internal and Rust-tolerant, Rust is fine despite the DSL cost. If consumers are generalist engineers, Python or TS win on adoption alone.
2. **Is real-time load-bearing?** 02 §2 (broadcast, presence, live metrics) is P2 — but if it's actually the vision (the spec reads like it wants it), Phoenix LiveView delivers it for free and nobody else does. If htmx-refresh is genuinely enough, it stops being a differentiator.
3. **Open-source ambition?** The doc set reads like a project with OSS ambitions. Python has the largest contributor pool for exactly this problem domain; Rust and TS attract the most framework-hacking contributors; Elixir the fewest.
4. **What's the deployment target?** A single static Rust binary is the nicest ops story; Django/Phoenix/Node all need a container runtime. Rarely decisive, but if ops has no container story, Rust is compelling.

---

## 8. Recommendation

**Default: Python/Django.** The spec's own values — declarative definitions, boring security, stand-up-fast, consumer ergonomics — map onto Django better than any other candidate. The framework still does all the interesting work (resource engine, actions, metrics, row-level policy, append-only audit, the 03 adapter layer, the UI shell); Django contributes the 60% of the boring list that is exactly the spec's "don't innovate on security plumbing." The system-checks mechanism preserves the "caught before deploy" promise at boot + CI. Iteration speed on the framework itself is the fastest, which matters for a build this size.

**Change the default when:**
- Real-time/presence is the vision, and the team will bet on BEAM → **Elixir/Phoenix LiveView**, after explicitly deciding the Ash relationship (build-on vs. compete).
- The building team is Rust-fluent and the consumer org is Rust-tolerant, and compile-time guarantees are the brand → **Rust/axum**, per [01](./01-rust-implementation.md) as designed.
- The consumer org is TypeScript-everywhere and adoption trumps security-plumbing effort → **TS/AdonisJS + htmx** (never Next/Remix).

**What I'd actually do:** build the reference implementation in Django, keep [01](./01-rust-implementation.md) on the shelf as the design for a future Rust port, and keep 03's adapter concepts in the doc as language-neutral interfaces (03 already reads that way). The spec docs (00/02/03) survive unchanged under any choice — only 01 is stack-bound, and it was written to port.

---

## 9. Consequences for existing docs

| Doc | If Django | If Elixir | If TS | If Rust |
| --- | --------- | --------- | ----- | ------- |
| 00-init | unchanged | unchanged | unchanged | unchanged |
| 01-rust-implementation | retained as port design | retained as port design | retained as port design | unchanged — this is the plan |
| 02-extra-features | unchanged (SSE via Channels/StreamingHttpResponse) | trivially satisfied (PubSub) | unchanged (SSE native) | unchanged |
| 03-data-adapter | unchanged; Django ORM is just one adapter | unchanged; Ecto is just one adapter | unchanged | unchanged |

The 03 taxonomy and capability model are the portability layer of the whole project. Whatever the reference stack, 03's §6 capability matrix is what keeps a later port honest.

---

## 10. Open questions

1. Team composition and consumer composition (deciding question 1) — unresolved here; this is the one the owners must answer.
2. Whether real-time is a P2 nice-to-have or the vision (deciding question 2).
3. OSS ambitions (deciding question 3) — the docs' tone suggests yes; it changes the calculus.
4. If Elixir: Ash relationship, and whether Twentytoo's 03 adapter taxonomy can coexist with Ash's data layer.
5. If Django: whether the framework layers on Django's auth/models directly or treats Django as one adapter among many (spec §4.1's `bind` suggests the latter; the two can coexist).
