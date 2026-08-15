# Template authoring guide

Built-in MiniJinja templates for the HTTP layer. This file is the style
contract for anyone (or any agent) adding or editing a `.j2` template: what
context is available, how escaping works, and — most importantly — how to keep
a template readable and how to split it into partials.

The engine, loader, and function/filter registry live in
`src/infrastructure/templates.rs`. The templates themselves live here at the
repo root (`web/templates/`) and are embedded into the binary by `build.rs`
via `minijinja_embed::embed_templates!`.

## Directory layout

```
templates/
  layout/     Full-page skeletons (`extends` targets).
  dashboard/  The home page and any non-resource top-level pages.
  resource/   The four generic resource views (list/detail/form).
  partials/   Reusable fragments pulled in with `{% include %}`.
```

- A **page** `extends` a layout and fills its `block`s.
- A **partial** is `include`d by a page or another partial.
- A **macro** is a parameterized snippet defined in one file and called
  elsewhere.

## Loader & validation model

Three sources, resolved in order (`src/infrastructure/templates.rs::TemplateEngine::new`):

1. **Built-ins** — compiled in by `minijinja_embed`. Invalid syntax fails the
   build, not the first request.
2. **User overrides** — a file in the override dir with the same name
   *replaces* a built-in.
3. **User-only templates** — loaded lazily through a path loader for names not
   in the binary.

`BUILTIN_TEMPLATES` (in `src/infrastructure/templates.rs`) lists every
template a handler renders; the boot check verifies each one resolves.
**When a handler renders a new template, add its name to
`BUILTIN_TEMPLATES`** so boot validation covers it.

## Static assets & design system

Built-in templates reference framework assets under `/static`, served from
the binary — the handler never reads the filesystem. Assets live in
`web/static/`, embed via `build.rs` into a name → bytes table, and are
resolved by `StaticFiles` (`src/infrastructure/static_files.rs`), which maps
file extensions to content types.

The stylesheet is five files loaded in order (01-ui-kit §5.1): `tokens.css`
(every `--tt-*` design token — the theming surface), `base.css`, `layout.css`,
`components.css`, `utilities.css`, plus the two scripts: `htmx.min.js`
(vendored) and `app.js` (the optional enhancement layer). The class names in
these templates are the design-language contract (01-ui-kit §7): reuse them,
don't invent siblings. Only `--tt-*` tokens — no raw hex values.

Rules:

1. A template references an asset as `/static/<name>` with `<name>`
   relative to `web/static/` (`css/tokens.css`, `js/app.js`).
2. **When a built-in template references a new asset, add its name to
   `StaticFiles::BUILTIN_ASSETS`** in `src/infrastructure/static_files.rs` —
   the boot check fails if a declared asset is missing from the binary.
3. Keep assets framework-owned and small; vendored third-party JS (htmx) is
   checked in under `web/static/js/` rather than loaded from a CDN.
4. Icons render through the `icon(name, size)` function — inline SVG from a
   closed set; adding an icon is a change to `ICON_NAMES`/`icon_paths` in
   `src/infrastructure/templates.rs`.

## Escaping (non-negotiable)

Autoescape is configured by file extension, not assumed:

- `.html.j2` → HTML-escaped automatically.
- anything else (`.j2`, `.txt.j2`, …) → raw.

Rules:

1. **HTML-emitting templates must be named `.html.j2`.** Partials that emit
   HTML are `.html.j2` too.
2. **Never `|safe` user or entity data.** The only "already-safe" strings are
   the return values of `format_field`, `format_filter`, and `form_control`,
   which escape internally — render them with a plain `{{ }}`, no `|safe`.
3. Don't hand-escape in the template (`|e` on top of autoescape double-escapes;
   a raw `{{ }}` on user data is an XSS hole). If you're tempted, the value
   probably belongs behind a framework function instead.

## Context variables

Handlers build the context with `minijinja::context!` (`src/handlers/`). Do
not reference keys that aren't listed — undefined variables render empty by
default (lenient), which silently hides typos.

Every page receives:

| Key | Type | Meaning |
| --- | --- | --- |
| `nav` | `Vec<NavItem>` | `key`, `label`, `icon` for the sidebar |
| `active` | `str` | active nav entry: `"home"` or a resource `key` |
| `actor` | `Actor` | `id`, `email`, `roles`, `permissions`, `team_id` |
| `auth` | `bool` | auth configured — gates the sign-out menu item |

Per template:

| Template | Extra keys |
| --- | --- |
| `dashboard/home.html.j2` | `cards` — `Vec<HomeCard>` (`key`, `label`, `icon`, `count`) |
| `resource/list.html.j2` + `partials/list.html.j2` | `resource`, `items`, `pager`, `q`, `has_filters`, `sort_param`, `link_base`, `can_create` |
| `resource/detail.html.j2` | `resource`, `record`, `can_update`, `can_delete` |
| `resource/form.html.j2` + `partials/form.html.j2` | `resource`, `mode`, `form_action`, `record_id`, `values`, `errors`, `form_error` |

`resource` is a `ResourceView` (`src/view.rs`): `key`, `label`, `columns`,
`detail_fields`, `form_fields`, `filters`, `sortable`, `searchable`. Each
`FieldView` has `name`, `label`, `kind` (`tag`, `options`, `relation`),
`required`, `sortable`.

Note: `can_create` / `can_update` / `can_delete` are **top-level** context
booleans, precomputed by the handler. They are *not* fields on `resource`.

## Functions & filters

Registered in `src/infrastructure/templates.rs`. Prefer these over reimplementing logic in
templates — that is their entire purpose.

| Call | Returns | Purpose |
| --- | --- | --- |
| `can("stores.create")` | `bool` | RBAC check over the `actor` in context |
| `icon("check", 16)` | safe HTML string | inline SVG from the closed icon set |
| `format_field(value, kind)` | safe HTML string | one cell/detail value for a field kind |
| `format_filter(filter)` | safe HTML string | sidebar control for one filter |
| `form_control(field, values)` | safe HTML string | form widget for one field |
| `value\|avatar_hue` | string | deterministic `avatar--<hue>` class for a name |

Rules:

- Never hand-write a `<select>` for a `select`/`badge`/`multiselect` kind,
  a badge span, a boolean `Yes`/`No`, a currency value, or a relation link.
  Call `format_field` / `form_control` / `format_filter` instead.
- `can()` reads the actor from render `State`, so templates call
  `can("stores.create")` — no actor argument.
- Anything data-shaping or non-presentational (joining labels, resolving
  relations, building URLs beyond a simple `/resources/{key}/{id}`) belongs in Rust, not
  in the template.

## Readability

1. **Two-space indent**, matching the existing files. No tabs.
2. **One construct per line** in `{% %}` tags; keep the HTML and the Jinja
   readable as separate lines. Break long tags at natural boundaries and keep
   the indentation meaningful.
3. **Comments explain why, not what.** Use `{# #}` only for non-obvious intent,
   decisions, or a pointer to a brainstorm section (`00 §…`), exactly like the
   existing templates. Don't restate the markup.
4. **Keep logic out of templates.** A template that grows an `{% if %}`
   decision tree over data shape, or builds strings with `~`, is doing Rust's
   job. Move it to a handler/view function and pass a precomputed value.
5. **One block does one thing.** `{% block content %}` should read top-to-bottom
   as an outline; pull each self-contained chunk into a partial.
6. **Don't over-escape or over-`default`.** Refer only to documented context
   keys; add `|default(...)` only when a key is genuinely optional (e.g. `q`).
7. **Whitespace control (`{%-` / `-%}`) is opt-in per line.** Don't blanket it;
   use it only where the rendered HTML would otherwise carry an unwanted blank
   line.

## Breaking files into partials

The goal is that **no single template exceeds ~150 lines** and every file has a
single responsibility. Three tools, three jobs:

| Tool | When | Example |
| --- | --- | --- |
| `{% extends %}` + `{% block %}` | Page skeleton: shared chrome, per-page slots | `layout/base.html.j2` |
| `{% include %}` | A fragment reused across pages, or a page section extracted for length | `partials/pagination.html.j2` |
| `{% macro %}` | A small, parameterized snippet called with explicit arguments | a repeated `<button>` or label block |

Extraction triggers:

- The same markup appears in **two or more** templates → partial.
- A page's `block` grows past ~40 lines → split the block's sections into
  partials.
- A fragment has a clear, nameable purpose (pagination, filter bar, field
  error, action buttons) → partial even if used once.

Conventions for partials:

1. Live in `partials/`, named for **what they render**:
   `pagination.html.j2`, not `list-footer.html.j2`.
2. HTML partials use the `.html.j2` extension (autoescape).
3. `{% include %}` uses the **full path from the template root**:
   `{% include "partials/pagination.html.j2" %}`.
4. A partial reads the same context as its caller — MiniJinja has no
   `with context`/`without context` modifier. Document any non-obvious
   dependency (e.g. `pagination.html.j2` requires `pager`) in a leading `{# #}`
   comment.
5. A partial should produce **one top-level element** and not assume surrounding
   chrome; the caller owns layout context.

Prefer `{% include %}` over `{% macro %}` when the fragment needs the render
context; prefer a macro when the snippet is parameterized and argument-driven.
MiniJinja macros take **explicit arguments only** (no `varargs`/`kwargs`).

## MiniJinja dialect notes

MiniJinja is close to Jinja2, but these are absent or different — avoid them:

- No `include` `with context` / `without context` modifiers (context always flows).
- No macro `varargs` / `kwargs` — explicit args only.
- No `continue` / `break` in loops (`loop_controls` is **not** enabled).
- No `%` string formatting (`"x %s" % v`) — use the `format` filter or `~`.
- No `date` filter — use the registered `format_datetime`.
- Python method syntax (`x.items()`) is unavailable — use `|items` style filters.
- Undefined variables are **lenient** by default (render empty) — see the
  context tables, not trial and error.

## Checklist before finishing a template change

- [ ] New/changed HTML template keeps the `.html.j2` extension.
- [ ] Any template a handler renders is in `BUILTIN_TEMPLATES`.
- [ ] Only documented context keys are referenced.
- [ ] Cell/form/filter rendering goes through `format_field` / `form_control` /
      `format_filter`, not hand-rolled markup.
- [ ] No `|safe` on user/entity data.
- [ ] Reusable or >40-line fragments are extracted into `partials/`.
- [ ] `cargo build` passes (syntax is validated at build time).
- [ ] Any asset a built-in template references is in
      `StaticFiles::BUILTIN_ASSETS`.
