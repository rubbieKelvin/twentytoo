# 02-extra-features.md — Beyond the MVP

**Status:** Pre-implementation brainstorm
**Depends on:** [00-init.md](./00-init.md) (core spec), [01-rust-implementation.md](./01-rust-implementation.md) (Rust design)
**Date:** 2026-08-11

---

## 0. Scope

The core spec covers the 80% bar — CRUD, RBAC, audit, flags, metrics. This document catalogs features that turn a competent internal-tool framework into a great one. Not all of these ship in v1. The goal is to design the extension points so these features can be layered on without rearchitecting.

Each feature is rated:

| Priority | Meaning |
| -------- | ------- |
| **P1**   | Ships in or immediately after MVP. The design must accommodate it from day one. |
| **P2**   | Natural follow-on. The extension points exist; it's implementation work, not design work. |
| **P3**   | Nice to have. Don't contort the architecture for it, but don't foreclose it either. |

---

## 1. Resource grouping (navigation organization)

**Priority:** P1

### 1.1 Problem

A dashboard with 30 resources becomes unusable if the sidebar is a flat alphabetical list. Users need to organize resources into logical groups — "Operations," "Finance," "User Management," "Configuration." This isn't just cosmetic; it's a navigational primitive that affects discoverability and RBAC (hide an entire group from a role).

### 1.2 Model

A **Group** is a named collection of resources, pages, and (optionally) nested sub-groups. It renders as a collapsible sidebar section with an icon and label.

```
group "operations" {
  label: "Operations"
  icon: "cog"
  items: [stores, orders, shipments]
  policy: requires("operations.view")   # hides entire group if user can't see ANY child
}

group "user_management" {
  label: "People"
  icon: "users"
  items: [users, roles_permissions, teams]
}

group "configuration" {
  label: "Config"
  icon: "sliders"
  items: [
    feature_flags,
    audit_log,
    group "integrations" {              # nested group
      label: "Integrations"
      icon: "plug"
      items: [webhooks, api_keys]
    }
  ]
}
```

### 1.3 Behavior

| Rule | Detail |
| ---- | ------ |
| **Visibility** | A group renders only if at least one child item is visible to the current actor (RBAC + flags). An empty group doesn't render — no "Operations" header with nothing under it. |
| **Collapse state** | Remembered per-user (localStorage for SSR, server-side preference later). Expanded by default. |
| **Active state** | A group auto-expands when the current route is inside it. The active child is highlighted. |
| **Ordering** | Groups are ordered as declared. Items within a group are ordered as declared. A `weight` field allows explicit ordering for edge cases. |
| **Badge** | Groups can show a count badge (e.g., "Operations (3 pending)") driven by a metric query. |
| **Flat fallback** | If no groups are defined, the sidebar renders all resources in declaration order — backward-compatible, zero migration cost. |

### 1.4 Integration with the module system

Groups can be declared in two places:

1. **In the consuming app** (top-level `Twentytoo::builder().with_group(...)`) — organizes resources from multiple modules into the navigation the team wants.
2. **In a module** — a module can declare a group that contains only its own resources. The consuming app can override or flatten this group.

The app-level declaration wins. If no app-level groups are defined, the framework auto-generates groups from module boundaries (one group per module) as a sensible default.

### 1.5 Sidebar rendering (conceptual)

```
┌─────────────────────┐
│ Dashboard    home   │
│                     │
│ ▼ Operations   3 ⏺  │  ← badge from metric query
│   Stores            │
│   Orders            │
│   Shipments         │
│                     │
│ ▶ People            │  ← collapsed
│                     │
│ ▼ Config            │
│   Feature Flags     │
│   Audit Log         │
│   ▶ Integrations    │  ← nested, collapsed
└─────────────────────┘
```

### 1.6 Implementation notes

- Groups are a **pure navigation concern** — they don't affect routing (URLs stay flat: `/stores`, `/orders`).
- The group hierarchy is resolved at startup into a `NavTree` and cached. It doesn't change at runtime (except for per-user visibility filtering).
- Nested groups are rendered with `<details>` / `<summary>` elements for the zero-JS case, enhanced with a few lines of Alpine.js for animation and state persistence when JS is available.

---

## 2. Broadcasting and broadcast channels

**Priority:** P2 (foundation in P2, richer use cases in P3-P4)

### 2.1 Problem

Internal tools are collaborative by nature. Two support agents shouldn't both process the same refund. An ops manager should see a metric tick up when a store manager approves something. The dashboard shouldn't require a manual refresh to reflect reality.

Broadcasting provides **server-to-client push** — the server tells connected clients "this resource changed" and the client updates in-place.

### 2.2 Model

A **channel** is a named pub/sub topic. The server publishes events to channels; connected clients subscribe to channels. Channels are hierarchical — subscribing to `stores` gets you events for `stores.*`, subscribing to `stores.42` gets you only record #42.

```
Channel                    Fires when
─────────────────────────────────────────────────────
stores                    Any store created/updated/deleted
stores.42                 Store #42 updated or deleted
stores.42.orders          Orders for store #42 change
orders?status=pending     Any order matching the filter changes
metrics.pending_approvals A metric value changes
audit                     Any audit entry is written
```

### 2.3 Transport: Server-Sent Events (SSE)

**SSE, not WebSockets**, for the default transport:

| Concern            | SSE                                        | WebSocket                       |
| ------------------ | ------------------------------------------ | ------------------------------- |
| Direction          | Server → client (unidirectional)           | Bidirectional                   |
| HTTP semantics     | Standard HTTP, works with proxies, HTTP/2  | Upgrade from HTTP, proxy issues |
| Reconnection       | Built-in (`Last-Event-Id`, auto-retry)     | Must implement manually         |
| Browser support    | Universal (`EventSource` API)              | Universal                       |
| What we actually need | Push updates to clients                | Nothing (actions are POSTs)     |

The dashboard sends actions via HTTP POST (forms). There is no client→server streaming requirement. SSE is the right tool.

That said, the transport should be behind a trait so a deployment that needs WebSockets (e.g., for a custom real-time collaboration page) can swap it:

```rust
#[async_trait]
pub trait BroadcastTransport: Send + Sync {
    /// Register a new client connection, returning a stream of events.
    async fn connect(&self, actor: &Actor, channels: &[String]) -> Result<EventStream>;

    /// Push an event to all subscribers of a channel.
    async fn publish(&self, channel: &str, event: BroadcastEvent);
}
```

### 2.4 Client-side integration

The base template includes a minimal SSE client (no library dependency — `EventSource` is native):

```html
<script>
(function() {
    const src = new EventSource("/_twentytoo/events?channels=stores,metrics.pending");
    src.addEventListener("resource.updated", (e) => {
        const data = JSON.parse(e.data);
        // If we're looking at the affected resource, refresh the table
        if (data.resource === currentResource) {
            htmx.trigger("#resource-table", "refresh");
        }
    });
    src.addEventListener("metric.changed", (e) => {
        const data = JSON.parse(e.data);
        htmx.trigger(`[data-metric="${data.key}"]`, "refresh");
    });
    src.addEventListener("notification", (e) => {
        const data = JSON.parse(e.data);
        showToast(data.message, data.level);
    });
})();
</script>
```

When JS is disabled, there's no live update — the user refreshes the page. This is consistent with the progressive enhancement principle.

### 2.5 Publishing events

Events are published from hooks and action handlers — no user code required for CRUD operations:

```rust
// In the generic update handler, after a successful update:
broadcast.publish(
    &format!("stores.{}", record.id),
    BroadcastEvent::resource_updated("stores", &record.id, &actor)
).await;

// Also publish to the parent channel for list-view subscribers:
broadcast.publish(
    "stores",
    BroadcastEvent::resource_updated("stores", &record.id, &actor)
).await;
```

Hooks allow custom publishing:

```rust
impl Resource for StoreResource {
    fn hooks(&self) -> Vec<Box<dyn Hook<Store>>> {
        vec![
            // After a store is approved, notify the regional dashboard
            Hook::after_update(|record, actor, broadcast| {
                if record.status == StoreStatus::Active {
                    broadcast.publish(
                        &format!("region.{}.stores", record.region_id),
                        BroadcastEvent::custom("store.activated", record)
                    );
                }
            })
        ]
    }
}
```

### 2.6 Use cases

| Use case | Channels | Trigger |
| -------- | -------- | ------- |
| Live-updating list views | `stores`, `orders` (resource-level) | Any CRUD operation on that resource |
| Live-updating metrics | `metrics.pending_approvals`, etc. | Metric value changes |
| Record detail refresh | `stores.42` (record-level) | That specific record is updated elsewhere |
| Toast notifications | `notifications.{user_id}` | Action completion, approval, error |
| Multi-user awareness | `presence.stores.42` | "User X is viewing this record" |
| Background job completion | `jobs.{job_id}` | Async action completes, CSV export ready |

### 2.7 In-memory vs. external broker

For single-process deployments (the default), events are fanned out in-process via `tokio::sync::broadcast`. No external dependency.

For multi-process deployments (behind a load balancer), a pluggable broker adapter bridges the gap:

```rust
#[async_trait]
pub trait BroadcastBroker: Send + Sync {
    async fn publish(&self, channel: &str, payload: &str);
    async fn subscribe(&self, channel: &str) -> Receiver<String>;
}

// Built-in implementations:
struct InProcessBroker { /* tokio::sync::broadcast */ }
struct RedisBroker { /* redis pub/sub */ }
struct PostgresBroker { /* LISTEN/NOTIFY */ }
```

MVP ships with `InProcessBroker` only. Redis and Postgres brokers are deferred until multi-process deployments are a real need.

---

## 3. Notification system

**Priority:** P2

### 3.1 Problem

Internal tools generate events that users need to know about: an approval came through, an export is ready, a scheduled task failed. Without notifications, users poll — refreshing pages, checking statuses manually.

### 3.2 Model

Three delivery channels, all driven from the same event source:

| Channel | Mechanism | Use case |
| ------- | --------- | -------- |
| **In-app toast** | SSE → browser toast | Immediate: "Store #42 approved" |
| **Notification center** | Persisted in DB, queried via SSE | Persistent: "3 new approvals since yesterday" |
| **Email** | Pluggable mailer | Async, offline: "Your export is ready" |

### 3.3 Notification resource

Notifications are themselves a built-in resource (like Users and Flags):

- **List view:** chronological feed with read/unread state, filterable by type
- **Bell icon** in the nav bar with unread count (live-updated via SSE)
- **Mark read** individually or in bulk
- **Preferences:** per-user settings for which notification types go to which channels
- **RBAC:** users see their own notifications; admins can see notification analytics (delivery rates, types)

### 3.4 Triggering notifications

```rust
// In an action handler:
action.execute(record, actor, input).await?;
notifications.send(
    Notification::info()
        .to_user(record.approver_id)
        .title("Doctor approved")
        .body(format!("{} approved Dr. {}", actor.email, record.name))
        .action_link(format!("/doctors/{}", record.id))
        .channel(NotifyChannel::all())  // in-app + email
).await;
```

Or declaratively, from a resource definition:

```rust
resource "stores" {
    // ...
    notifications: {
        after_create: NotificationRule {
            template: "A new store '{{ record.name }}' was created",
            recipients: ["role:admin"],
            channel: InApp,
        },
        after_update: NotificationRule {
            condition: |record| record.status == StoreStatus::Suspended,
            template: "Store '{{ record.name }}' was suspended by {{ actor.email }}",
            recipients: ["record.owner_id", "role:admin"],
            channel: All,
        },
    }
}
```

---

## 4. Saved filters and bookmarks

**Priority:** P2

### 4.1 Problem

Operators build the same filtered views repeatedly — "pending stores in the Northeast region," "high-value orders from the last 7 days." Without saved filters, this is a daily ritual of re-applying the same dropdowns and search terms.

### 4.2 Model

A **SavedFilter** is a named, shareable snapshot of the current list view state:

```
saved_filter "pending_northeast_stores" {
  resource: stores
  name: "Pending — Northeast"
  filters: { status: pending, region: northeast }
  sort: created_at desc
  columns: [name, owner, created_at]
  shared_with: [role:support]   # empty = private
}
```

### 4.3 Behavior

- Saved filters appear as quick-select chips or a dropdown above the table.
- Selecting one applies its filters, sort, and column set to the current view.
- The URL updates (`/stores?filter=pending_northeast_stores`) so filtered views are shareable via link.
- Users can star/bookmark filters for their personal sidebar. Starred filters appear under the resource in the nav as sub-items.
- Filters are created from the current view state (a "Save current view" button).

---

## 5. Scheduled tasks

**Priority:** P3

### 5.1 Problem

Some internal-tool operations should run on a schedule: "every morning at 8am, generate and email yesterday's order summary," "every hour, check for stale pending approvals and escalate."

### 5.2 Model

A **ScheduledTask** is a named job with a cron expression and a handler:

```
scheduled_task "daily_order_summary" {
  schedule: "0 8 * * *"
  policy: requires("orders.export")
  handler: fn(ctx) -> Result
  on_failure: notify("role:admin")
}
```

Scheduled tasks are managed via a built-in resource:
- **List view:** all tasks with their schedule, last run time, last status
- **Manual trigger** button ("Run now")
- **Pause/resume** toggle per task
- **Execution history** (runs are audit-logged)

### 5.3 Implementation

For single-process deployments: a `tokio` task loop that checks a schedule table every 30 seconds. For multi-process: a `SKIP LOCKED` query ensures only one instance picks up each task. No external scheduler dependency (no Sidekiq, no Celery) required for the baseline.

---

## 6. API keys and programmatic access

**Priority:** P3

### 6.1 Problem

As internal tools mature, teams want to automate operations: a CI pipeline that triggers an action, a script that exports data, a webhook receiver that creates records. The dashboard needs a programmatic interface that isn't session-cookie-based.

### 6.2 Model

An **ApiKey** is a scoped, revocable credential:

```
api_key "ci_export_key" {
  label: "CI order export"
  scopes: ["orders.view", "orders.export"]
  expires_at: 2026-12-31
  created_by: actor.id
}
```

- API keys are managed via a built-in resource (create, revoke, rotate).
- Authentication: `Authorization: Bearer twentytoo_key_xxxx` header.
- Keys inherit the permissions of their scopes — same RBAC engine, different auth mechanism.
- Every API key usage is audit-logged with the key label, not just the key ID.
- Keys can be restricted to IP ranges for additional security.

---

## 7. Dashboard customization

**Priority:** P3

### 7.1 Per-user dashboard layout

The dashboard home is a grid of metric cards. Users should be able to:
- Show/hide specific metrics
- Reorder cards via drag-and-drop (progressive enhancement — a settings form when JS is off)
- Resize cards (small/medium/large)
- Create personal dashboards (separate tabs/views for different workflows)

Layout preferences are persisted per-user in the database. The default layout is defined declaratively in the app configuration; user overrides are merged on top.

### 7.2 Role-based default dashboards

Different roles see different default dashboards:

```
dashboard "support_default" {
  role: support
  metrics: [open_tickets, avg_response_time, tickets_by_category]
}
```

A `support` user who hasn't customized their dashboard sees this layout. An `admin` sees a different default. A user who has customized sees their personal layout regardless of role.

---

## 8. Comments and collaboration

**Priority:** P3

### 8.1 Problem

Record-level decision-making often involves discussion: "Should we approve this doctor given the credential gap in section 4?" This discussion currently happens in Slack, email, or not at all — disconnected from the record it's about.

### 8.2 Model

A **Comment** is a timestamped, authored note attached to a record:

```
comment {
  record: doctors.42
  author: actor.id
  body: "Credential gap is minor — previously discussed with medical board. OK to approve."
  created_at: 2026-08-11T14:30:00Z
}
```

- Comments appear in a tab on the record detail view.
- Support `@mentions` that notify the mentioned user (in-app + email).
- Comments are audit-logged (the comment itself *is* an audit event — "User X commented on Doctor #42").
- Markdown body for rich formatting.
- Comments are **not editable** after posting (append-only, like audit log).

### 8.3 Threading

Comments can be threaded (reply to a comment). Single level of nesting — replies to replies are flattened under the parent. Deep threading is a P3 anti-goal; if you need a full discussion system, use a custom page + Slack integration.

---

## 9. Import and export enhancements

**Priority:** P2

### 9.1 Beyond CSV

The core spec covers CSV export of the current filtered view. Enhancements:

- **Excel (.xlsx) export** — multi-sheet workbooks (one sheet per resource), formatted headers, auto-filter rows, column widths. This is what business users actually want.
- **Import from CSV/Excel** — create or update records in bulk. Map columns → fields interactively. Preview changes before committing. Dry-run mode (validate only).
- **Export templates** — predefined export configurations ("Monthly financial report" = orders + revenue + refunds, specific columns, specific date range).
- **Scheduled exports** — "Email me this report every Monday at 9am."

### 9.2 Import workflow

1. Upload file (CSV, XLSX, or JSON)
2. Preview: show first 10 rows with mapped columns
3. Map: user maps file columns to resource fields (auto-map by header name match)
4. Validate: run validation rules on every row, show errors
5. Commit: insert/update records in a transaction (or row-by-row with error skipping)

The import workflow is a custom Page in the framework's UI — a multi-step wizard that composes form components, table components, and the DataAdapter trait.

---

## 10. Theme and branding

**Priority:** P2

### 10.1 Design tokens

The visual shell is controlled by a `Theme` struct, not by editing CSS:

```rust
pub struct Theme {
    pub name: &'static str,
    pub logo_url: Option<String>,
    pub favicon_url: Option<String>,

    pub colors: ThemeColors,
    pub typography: ThemeTypography,
    pub spacing: ThemeSpacing,
    pub border_radius: BorderRadius,
    pub density: Density,  // Comfortable | Compact
}

pub struct ThemeColors {
    pub primary: &'static str,       // "#4F46E5"
    pub primary_foreground: &'static str,
    pub background: &'static str,
    pub surface: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    pub border: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
}
```

The framework ships with a default theme (Tailwind-inspired neutral palette) and a dark variant. Custom themes are defined in the consuming app's `main.rs` and injected into the template context. The built-in CSS uses CSS custom properties generated from the `Theme` struct, so a custom theme is a different set of variable values — no CSS editing needed.

### 10.2 Dark mode

Per-user preference (`light` | `dark` | `system`). The preference is stored server-side and applied via a `data-theme` attribute on `<html>`. The toggle is in the user dropdown menu. When JS is disabled, the preference is applied server-side on the next full-page load.

---

## 11. Quick reference: feature phase mapping

| Feature                    | Priority | Phase   | Blocked by                |
| -------------------------- | -------- | ------- | ------------------------- |
| Resource grouping          | P1       | Phase 1 | Nothing — design it in    |
| Broadcasting (SSE)         | P2       | Phase 2 | Resource engine stability |
| Notification system        | P2       | Phase 3 | Broadcasting              |
| Saved filters              | P2       | Phase 3 | List view stability       |
| Import/export enhancements | P2       | Phase 3 | Actions (async)           |
| Theme + branding           | P2       | Phase 4 | Template stability        |
| Dashboard customization    | P3       | Phase 4 | Metrics stability         |
| Comments                   | P3       | Phase 4 | Broadcasting              |
| API keys                   | P3       | Phase 4 | RBAC stability            |
| Scheduled tasks            | P3       | Phase 4 | Async actions             |

---

## 12. Deliberately excluded

Features that were considered and rejected (at least for now):

- **Workflow/approval chains:** Multi-step, multi-actor approval flows (e.g., "Manager approves → Director approves") are a state machine problem. The escape hatch is a custom Page with the framework's UI primitives. Building a workflow engine into the core is a separate product.
- **Drag-and-drop resource builder:** No-code is an explicit non-goal (see 00-init.md §3). Resources are defined in code.
- **Public-facing portals:** Customer-facing dashboards (e.g., a store manager's self-service portal) require a different auth model, different design language, and different performance characteristics. Use the framework to build the *internal* tool that manages the data; use a separate stack for customer-facing views.
- **Native mobile app:** Internal tools on mobile are a progressive-web-app problem (responsive templates + service worker). No native app wrapper.
- **Multi-language UI (i18n):** Deferred. The framework's built-in strings (labels, buttons, error messages) are in English. i18n hooks (string extraction, translation file format) are designed into the template layer so a community contribution can add it, but the reference implementation doesn't ship with translation infrastructure.
- **GraphQL API:** The framework serves HTML. For programmatic access, a REST-ish JSON API is sufficient for the API key use case. GraphQL adds complexity (batching, dataloader, N+1 prevention) without proportional benefit for internal tools.
