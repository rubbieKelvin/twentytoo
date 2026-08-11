# 00-init.md — Internal Tools Dashboard Framework

**Working name:** Twentytoo
**Status:** Pre-implementation brainstorm
**Date:** 2026-08-11

---

## 0. Purpose

Every engineering team that operates a non-trivial product eventually needs internal tools — a support dashboard, an ops panel, an approval queue, a customer-lookup console. And every team solves the same problems from scratch: auth, RBAC, tables with filtering/sorting/pagination, forms with validation, audit logging, CSV exports. The result is a proliferation of half-built, inconsistently-styled, under-permissioned admin panels that rot the moment the original author moves on.

Twentytoo exists to make that whole class of work **declarative rather than imperative**. Instead of building CRUD screens for your Nth internal entity, you define the entity — its fields, its policies, its actions — and the framework does the rest. The team spends its time on domain logic, not on table wiring.

The measure of success: standing up a new internal tool for a new domain should require writing **domain logic and resource definitions only** — never re-solving auth, RBAC, tables, forms, filtering, pagination, audit logging, or navigation.

---

## 1. Vision

A **server-rendered, framework-agnostic starter kit** that lets a team stand up a secure, permissioned internal dashboard for *any* domain — healthcare ops, e-commerce back office, support tooling, finance ops, logistics — by **declaring** resources, metrics, actions, and pages, rather than hand-building CRUD screens per project.

The framework ships as a **library**, not a template. You don't fork it and drift; you compose it into your app, register modules, and override what you need to override. Upstream improvements are a dependency bump away.

**Scope:** Stack-agnostic core spec. Rendering model is fixed to **server-side rendering (SSR)** with optional progressive-enhancement JS (htmx-style partial swaps, Alpine-style local state). No SPA build step is required to use the framework. HTML over the wire, always.

---

## 2. Design principles

- **SSR-first, progressively enhanced.** The dashboard works with JS disabled. Partial-page updates (filtering, sorting, form validation) are enhancements, not requirements.
- **Convention over configuration for the 80% case; escape hatches for the 20%.** Anything that fits the resource/action/metric/page model should require zero boilerplate. Anything that doesn't gets a clean custom-page/handler escape hatch that still reuses the same UI primitives (tables, cards, forms, badges) as generated resource views.
- **Declarative, not generated.** Resources are defined once as data/config/traits — not scaffolded into files that drift from a generator template. The definition IS the source of truth; there is no generated code to fall out of sync.
- **RBAC and audit logging are first-class**, not an afterthought bolted on in phase 3. Every view, every button, every field renders or doesn't render based on policy, and every mutation is logged.
- **Single-tenant by default, multi-tenant-ready.** Row-level scoping is part of the core policy model, even if a given deployment never uses it.
- **Core concepts are stack-agnostic.** The spec below should be implementable in Rust, Elixir/Phoenix, Ruby/Rails, Python/Django, PHP/Laravel, Go, etc. A reference implementation picks one stack; the *interfaces* described here should port. The constraints of the reference stack should not leak into the spec.
- **Boring where it counts.** Auth, session management, CSRF, SQL injection prevention, XSS — solved problems. Use battle-tested libraries; don't innovate on security plumbing.

---

## 3. Non-goals

- **Not a no-code builder.** This is a framework for engineers, not a drag-and-drop tool for non-technical users.
- **Not a BI/analytics platform.** Metrics are operational glance-and-act indicators, not a query workbench or data warehouse UI.
- **Not a public-facing product UI toolkit or marketing site builder.**
- **Not trying to replace bespoke customer-facing UX** — this is for internal tools only.
- **Not a headless CMS or content authoring tool.** The data model is operational entities, not blog posts or landing pages.
- **Not a workflow/automation engine** (at least not in v1). Actions are discrete, user-triggered operations, not a DAG of automated steps.

---

## 4. Core abstractions

Six primitives cover the framework's surface area:

| Primitive  | Answers                                                                 |
| ---------- | ----------------------------------------------------------------------- |
| **Resource** | "What entity am I managing?" (users, orders, doctors, stores)         |
| **Field**    | "What does one piece of data on that entity look like, and where does it show up?" |
| **Action**   | "What can a user *do* to a record (or set of records) beyond CRUD?"   |
| **Metric**   | "What number/trend/breakdown matters at a glance?"                    |
| **Page**     | "What doesn't fit the resource model at all?"                         |
| **Policy**   | "Who is allowed to do any of the above, and to which records?"        |

### 4.1 Resource

The core unit. A Resource binds a data source (table/collection/external API) to a declarative definition of how it's displayed, filtered, searched, sorted, and mutated.

```
resource "stores" {
  bind: StoreEntity              # underlying model/table
  label: "Stores"
  icon: "storefront"

  fields: [...]                  # see 4.2
  list_columns: [name, owner, status, created_at]
  default_sort: created_at desc
  search_fields: [name, owner_email]
  filters: [status, plan_tier, created_at_range]

  relationships: {
    customers: has_many(Customer, via: store_id)
    locations: has_many(Location, via: store_id)
  }

  actions: [...]                 # see 4.3
  metrics: [...]                 # see 4.4, scoped to this resource's detail page

  policy: StorePolicy            # see section 5
  hooks: {
    before_create, after_update, before_delete
  }
}
```

A Resource automatically gets, with zero extra code:

- **List view:** paginated table with search, filter, sort, column visibility toggle
- **Detail view:** field display + related-resource tabs (e.g. a store's customers, inline)
- **Create / edit forms:** generated from field definitions, with client+server validation
- **Bulk selection + bulk actions** (delete, status change, export)
- **CSV/Excel export** of the current (filtered) view
- **Per-record audit history tab** (who changed what, when)

### 4.2 Field

```
field name          { type: text,     required: true, list: true,  form: true }
field email         { type: email,    required: true, list: true,  form: true, searchable: true }
field status        { type: badge,    options: [pending, active, suspended], list: true }
field revenue_ytd   { type: currency, computed: true, list: true,  form: false }
field logo          { type: image,    form: true, list: false }
field owner         { type: relation, target: users, list: true }
field notes         { type: richtext, form: true, list: false, visible_to: [admin, support] }
```

Supported types (v1 baseline): `text`, `textarea`, `richtext`, `number`, `currency`, `boolean`, `select`, `multiselect`, `date`, `datetime`, `email`, `file`, `image`, `relation`, `badge/status`, `json`, `computed`.

Every field can declare:

- **Context visibility** — which of `list` / `detail` / `create` / `edit` it appears in
- **Role visibility** — who can see or edit it (field-level RBAC, not just resource-level)
- **Validation** — required, format, min/max, custom validator hook
- **Searchability** — whether the field participates in the resource's global search
- **Sortability** — whether the column is sortable in list view

### 4.3 Action

An Action is anything beyond plain field mutation: approving a doctor, suspending a store, resending an invite, exporting a filtered report. Actions are the verbs of the system — CRUD gives you nouns, Actions give you domain-specific verbs.

```
action "approve_doctor" {
  scope: record            # record | bulk | standalone
  resource: doctors
  label: "Approve"
  requires_confirmation: true
  input_fields: []          # none needed
  policy: requires("doctors.approve")
  handler: fn(record, actor, params) -> Result
  execution: sync           # sync | async
}

action "reject_doctor" {
  scope: record
  resource: doctors
  label: "Reject"
  input_fields: [ field reason { type: textarea, required: true } ]
  policy: requires("doctors.approve")
  handler: fn(record, actor, params) -> Result
  execution: sync
}

action "export_orders" {
  scope: bulk
  resource: orders
  label: "Export selected"
  policy: requires("orders.export")
  execution: async          # queued, user gets a download-ready notification
}
```

Actions render as buttons on the record row, the record detail page, or (for bulk/standalone) the list toolbar / dashboard home. Critical rule: **an action a user can't perform doesn't just get disabled — it doesn't render at all.** No grayed-out buttons leaking information about what's possible.

### 4.4 Metric

Operational at-a-glance numbers, attachable to the dashboard home or to a specific resource's detail page.

```
metric "pending_doctor_approvals" {
  type: value               # value | trend | partition | table | progress
  query: count(doctors where status = "pending")
  refresh: 60s
  link_to: doctors?status=pending
}

metric "orders_last_30d" {
  type: trend
  query: count(orders) grouped_by day, range: 30d
  attach_to: dashboard_home
}

metric "orders_by_store" {
  type: table
  query: top(orders grouped_by store_id, limit: 10)
  attach_to: resource(stores).detail   # shown on each store's own page, scoped to that store
}
```

Metric types:

| Type        | Visual                                | Use case                                    |
| ----------- | ------------------------------------- | ------------------------------------------- |
| `value`     | Single number + label + delta         | "Pending approvals: 12"                     |
| `trend`     | Sparkline or small line chart         | "Orders last 30 days"                       |
| `partition` | Donut/bar breakdown                   | "Orders by status"                          |
| `table`     | Compact ranked table                  | "Top 10 stores by revenue"                  |
| `progress`  | Progress bar with target              | "Onboarding completion: 67%"                |

### 4.5 Page (escape hatch)

For anything that isn't record CRUD or a self-contained action — a multi-step review queue, a map view, a custom report builder.

```
page "doctor_review_queue" {
  route: "/doctors/review-queue"
  nav_label: "Review queue"
  policy: requires("doctors.approve")
  handler: fn(ctx) -> render(...)   # full custom controller logic
}
```

Pages still compose from the same UI primitives (tables, cards, forms, badges) as generated resource views, so they're visually consistent without reusing the generic CRUD machinery. The escape hatch is a pressure release valve — it should be used deliberately, not as the default.

### 4.6 Policy

See Section 5 in full. In short: every Resource, Action, and Page declares a policy requirement; policies resolve against the current actor's roles/permissions and, optionally, the specific record.

---

## 5. RBAC specification

RBAC is not a plugin — it's load-bearing for every other primitive. A page that renders without checking permissions is a bug, not a missing feature.

### 5.1 Model

- **User** — an authenticated actor
- **Role** — a named bundle of permissions (`admin`, `support`, `store_manager`, `doctor_reviewer`)
- **Permission** — a `resource.action` pair (`stores.view`, `stores.delete`, `doctors.approve`, `*.view`)
- **Team/Org** *(optional, multi-tenant deployments)* — a scoping boundary a user and a set of records both belong to

### 5.2 Permission checks — four enforcement points

1. **Navigation** — nav items for resources/pages the user has no `view` permission on don't render.
2. **Route/handler guard** — every generated and custom route checks policy before executing, independent of UI. You cannot bypass a permission check by typing a URL.
3. **UI element visibility** — action buttons, edit forms, and delete controls render only if the specific policy check passes for that record. No disabled buttons, no "you don't have permission" after the fact — the option simply isn't there.
4. **Field visibility** — individual fields can be hidden or shown read-only per role, independent of resource-level access (e.g. `support` sees a customer's masked payment info; `admin` sees it in full).

### 5.3 Row-level (record) scoping

Beyond "can this role do X on this resource," policies must answer "can this **user** do X on this **specific record**." This is what makes the framework usable for the multi-tenant e-commerce case (a store manager should only manage *their* store) as much as the healthcare case (a reviewer might be scoped to a specialty or region).

```
policy StorePolicy {
  view(actor, record):   actor.role == admin || record.owner_id == actor.id
  update(actor, record): actor.role == admin || record.owner_id == actor.id
  delete(actor, record): actor.role == admin
}
```

Row-level policies compose with role-level permissions: the role check gates the *capability*, the row check scopes the *data*. Both must pass.

### 5.4 Runtime-editable roles

Roles/permissions are seedable at deploy time but also editable **without a redeploy**, via a built-in `Roles & Permissions` resource — itself governed by RBAC (only an `admin`-equivalent role can edit roles). This is what lets a company adapt the tool to their org chart without touching code.

### 5.5 Audit log

Every create, update, delete, and action invocation is logged: actor, timestamp, resource + record id, before/after diff (where feasible), and request context (IP, session). Audit entries are queryable per-record (shown as a tab on the detail view) and globally (its own resource, itself RBAC-gated).

The audit log is **append-only and immutable** by design. No admin can delete or modify audit entries — if a deployment needs retention policies, that's handled at the storage layer, not the application layer.

---

## 6. User management (built-in)

Ships as a first-class built-in module, not left to each deployment to rebuild:

- **Users resource:** invite flow (email + role assignment), email verification, password reset, deactivate/reactivate, role assignment, last-login tracking.
- **Pluggable auth provider interface** — reference implementation covers session + password; the interface is designed to extend to SSO/OIDC/SAML without touching the rest of the framework.
- **Session management:** list a user's active sessions, force logout (individual or all).
- **Self-service profile page:** name, avatar, password change, 2FA if enabled.
- **Impersonation** ("view as user X"): common internal-tool need for support/admin debugging, gated behind an explicit permission (`users.impersonate`) and always written to the audit log with both the impersonator and the target recorded.

---

## 7. Extensibility model

- **Modules** bundle a set of resources + metrics + pages + nav entries + (if applicable) migrations, and register into the host app at startup. A company's domain-specific work — "doctor approval," "store management" — is a module, not a fork of core.
- **Core is a library**, not a template you copy-paste and diverge from. The consuming app is a thin composition layer: register modules, set theme tokens, done.
- **Theming** via design tokens (color, spacing, logo, typography) — the visual shell is swappable without touching generated-view logic.
- **Hooks** for cross-cutting concerns: `before_save`, `after_update`, `on_action_executed` — used for things like webhook emission, notification dispatch, or denormalized field updates.
- **Render overrides:** every core UI primitive (table cell renderer, form field renderer, nav item, badge) supports a per-field or per-resource override, so a company can customize one rendering without forking the table/form engine itself.

---

## 8. Coverage checklist — the "80% of internal tools" bar

The framework should need **zero custom page/handler code** for:

- [ ] Standard CRUD for any entity
- [ ] Search, filter, sort, pagination on any list view
- [ ] Bulk actions (bulk status change, bulk delete, bulk export)
- [ ] Record-level approval/review actions with a reason/comment input
- [ ] Relationship browsing (viewing a parent record's related children inline)
- [ ] File/image upload and preview
- [ ] CSV/Excel export of any (filtered) resource view
- [ ] A dashboard home composed of metrics
- [ ] Per-record audit history
- [ ] Role-based access, including row-level/record scoping
- [ ] User invite, deactivation, and role management
- [ ] In-app and/or email notifications on key events
- [ ] Global search across resources

Custom pages are the intentional, expected escape hatch for the remaining ~20% — e.g. a multi-step review queue with its own state machine, a map view, or a domain-specific report builder.

---

## 9. Architecture layers (stack-agnostic)

| Layer               | Responsibility                                                        | Portability requirement                                                    |
| ------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| **Data adapter**    | query, filter, paginate, sort, aggregate, transact                    | Must work against any ORM/data layer via a thin adapter interface          |
| **Resource engine** | resource registry + generic list/detail/create/edit/delete handlers   | Framework-owned, stack-specific implementation                             |
| **Presentation**    | server-rendered table/form/card/badge/nav components + minimal JS     | Should map onto any templating engine (Tera, Askama, ERB, Blade, EEx, etc.) |
| **Auth/RBAC**       | pluggable auth provider + policy engine                               | Policy engine is pure logic; auth provider is swappable                    |
| **Job/queue**       | backing for async actions and exports                                 | Pluggable adapter (in-process for small deployments, real queue for scale) |
| **Notifications**   | in-app / email / webhook dispatch on events                           | Pluggable adapter                                                          |

---

## 10. Reference prior art

Django admin, Rails (ActiveAdmin/Avo), Laravel (Nova/Filament), Refine.dev, and Retool all solve pieces of this. Here's how Twentytoo differs:

| Tool            | Model              | JS required? | Row-level RBAC? | Stack-portable spec? |
| --------------- | ------------------ | ------------ | --------------- | -------------------- |
| Django admin    | Declarative, Python | No           | No (add-on)     | No (Django-only)     |
| ActiveAdmin     | DSL, Ruby          | No           | Via gems        | No (Rails-only)      |
| Filament        | Declarative, PHP   | No (Livewire)| Via policies    | No (Laravel-only)    |
| Refine.dev      | React components   | Yes (SPA)    | Via provider    | No (React-only)      |
| Retool          | Drag-and-drop      | Yes          | Via UI config   | N/A (platform)       |
| **Twentytoo**   | Declarative, SSR   | **No**       | **First-class** | **Yes** (by design)  |

Twentytoo's differentiators: **SSR-first** (no required JS build/runtime), **framework-agnostic core spec** rather than tied to one language's ecosystem, and **row-level RBAC as a first-class primitive** rather than an add-on.

---

## 11. MVP phasing

### Phase 1 — Foundation
Resource engine (CRUD, fields, list/detail/form generation), role+permission RBAC (no row-level yet), built-in user management, audit log.

**Deliverable:** You can define a resource, get generated CRUD views with role-gated access, manage users, and see who changed what.

### Phase 2 — Actions & Metrics
Actions (sync only), metrics (`value` + `trend` types), custom pages.

**Deliverable:** You can attach domain-specific buttons to records, show operational numbers on a dashboard, and build one-off pages that don't fit the resource mold.

### Phase 3 — Row-level & Async
Row-level/record policies, bulk + async actions, exports, notifications, impersonation.

**Deliverable:** Multi-tenant scoping works. Long-running operations don't block the request. Users can export data and get notified.

### Phase 4 — Polish & Distribution
Module packaging/distribution, theming system, global search, real-time updates (SSE/WebSocket for live tables and metrics).

**Deliverable:** The framework is distributable, themeable, and feels like a product rather than a scaffold.

---

## 12. Open questions to resolve before implementation

- **Reference stack:** which language/framework does the first implementation target? Given the current repo context: **Rust / Axum / Tera** (or Askama) is the natural first target, with this spec used to keep the design portable. The constraint: Rust's type system and compile-time guarantees are an asset for a framework that wants to catch misconfiguration at build time, but may slow the iteration speed of the reference implementation relative to a dynamic language.
- **Multi-tenancy:** core primitive from day one, or a Phase-3+ module? Leaning toward: design the policy model to accommodate it from the start (the `actor.team_id == record.team_id` pattern should work without restructuring), but defer the Team/Org management UI and invitation flow to Phase 3.
- **Real-time updates** (WebSockets/SSE for live-updating tables/metrics): in scope for v1, or deferred to Phase 4?
- **i18n:** in scope for v1? Leaning toward: no — design the string extraction points so i18n can be layered on later without rearchitecting, but don't ship translation infrastructure in the MVP.
- **Module distribution:** monorepo/workspace only initially, or design for out-of-repo module installation (e.g., a registry, Git-based install) from the start?
- **Database support:** start with PostgreSQL only (the most feature-rich target for things like row-level policies pushed to the DB), or design the data adapter to support SQLite/MySQL from day one?
- **Form builder depth:** generated forms cover 80% of cases, but how much layout control (field grouping, conditional visibility, multi-step wizards) should the field definition DSL support before we tell people "use a custom page"?

---

## 13. Worked examples (grounding the spec)

### MobiHealth (telemedicine platform)

**Entities:** doctors, prescriptions, patients, appointments, reviews.

- `doctors` and `prescriptions` are Resources with standard CRUD.
- "Approve doctor" and "Reject doctor" are record-scoped Actions with a `doctors.approve` policy.
- "Pending approvals" is a `value` Metric on the dashboard home — the ops manager sees it the moment they log in.
- A more involved multi-step credential review (collect documents → verify → approve/reject with comments) becomes a custom Page (`/doctors/review-queue`) because it has its own state machine that doesn't fit a single Action.
- Row-level scoping: a regional reviewer can only see and approve doctors in their assigned region.

### Ringroad (multi-tenant e-commerce platform)

**Entities:** stores, customers, locations, orders, products.

- `stores`, `customers`, `locations` are Resources.
- `stores` has a row-level policy: `store_manager` role can only view/update stores where `store.owner_id == actor.id`. An `admin` can see all.
- "Suspend store" is a record-scoped Action with a confirmation dialog.
- "Orders by store, last 30 days" is a `table` Metric attached to each store's detail page (not the global dashboard) — scoped automatically to the store being viewed.
- "Export orders" is a bulk Action that queues a CSV generation job and notifies the user when ready.
- A store manager logs in and sees only their store, their customers, their orders — the multi-tenancy is invisible, not something they have to filter manually.

---

## 14. Immediate next steps

1. **Pick the reference stack** and scaffold the project.
2. **Implement the Resource engine** end-to-end for one resource type (probably `users` since it's built-in anyway) — list view, detail view, create/edit forms, all server-rendered.
3. **Prove the data adapter interface** by making that one resource work against a real database.
4. **Layer on RBAC** (role + permission, not row-level yet) and verify that all four enforcement points work on the single resource.
5. **Ship the audit log** — if every mutation is logged from day one, we never have to retrofit it.
6. **Then expand** to a second resource, then actions, then metrics.

The guiding rule for the reference implementation: **build the framework by using it.** Every phase should produce a working dashboard for at least two resources. If building feature X requires touching code that isn't the feature itself (reworking the table component to support a new field type, for instance), that's a sign the abstractions need adjustment.
