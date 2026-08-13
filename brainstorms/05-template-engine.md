# 05-template-engine.md — Jinja2 (MiniJinja) as the template engine

**Status:** Brainstorm — decision recorded
**Depends on:** [00-init.md](./00-init.md) (core spec), [01-rust-implementation.md](./01-rust-implementation.md) (§3, §4, §6, §8, §10, §12), [02-extra-features.md](./02-extra-features.md), [03-data-adapter.md](./03-data-adapter.md), [04-framework-language.md](./04-framework-language.md)
**Date:** 2026-08-12

---

## 0. The decision

Two decisions are recorded here, in order of specificity:

1. **Rust is the reference implementation language.** The owner has chosen the Rust path from [04](./04-framework-language.md) §2 over that doc's Django default. 04's deciding question 1 (who builds, who consumes) is thereby answered for this project: the building team is Rust-fluent and accepts the consumer ergonomics of 01's builder surface. 04's remaining open questions (real-time ambition, OSS, deployment target) are untouched and don't block implementation.
2. **The template engine is Jinja2 — concretely, MiniJinja — not Tera.** 01 §3 chose Tera; this doc replaces that choice and everything downstream of it (01 §3.2/§3.3, §4.2, §4.4, §6.3, §8.1, §8.2, §10.3, §10.4, §12).

"Jinja in Rust" means exactly one crate: **`minijinja`** (v2.x — 2.23.0 at time of writing). It is a native Rust implementation of the *Jinja2 language* — the same template language as Django, Flask, Ansible, dbt, and Airflow — maintained by Armin Ronacher, the author of Jinja2 itself. There is no credible alternative for "Jinja2 syntax compiled to native Rust": the other Jinja-flavored Rust engines (Tera, Askama) have deliberately diverged from Jinja2, and embedding real CPython Jinja2 via PyO3 would destroy the single-static-binary deployment story that 01 §0 lists as a core benefit.

### Why this doc exists

- 01 §3's decision table is Tera vs. Askama — it never considered the third member of the Jinja family. The real v1 question was "which Jinja-family engine," and this doc re-runs that comparison with MiniJinja included.
- 04 §2 called Tera "serviceable but the dev loop is the weakest of the four" candidates. MiniJinja's ecosystem (autoreload, CLI, playground, the whole Jinja2 editor toolchain) answers part of that criticism directly; the rest is honest engineering work, listed in §10.
- Everything else in 00–04 is engine-agnostic: 03 §3.1's "view layer touches entities only as serialized JSON," 02's export/email/SSE template needs, 00's rendering model. None of it cares whether the renderer is Tera or MiniJinja. The template engine is a swap-in component, and this doc is the swap.

---

## 1. What MiniJinja is

Verified facts (project README, COMPATIBILITY.md, benchmarks, and source, at v2.23.0):

| Fact | Detail |
| --- | --- |
| Language | Jinja2 syntax and behavior; the project's stated goal is to "stay as close as possible to Jinja2" and to "leverage an already existing ecosystem of editor integrations" |
| Maintainer | Armin Ronacher — Jinja2's author. The language cannot drift from Jinja2 the way Tera has drifted; COMPATIBILITY.md is the deviation ledger |
| Dependency footprint | `serde` is the *only* required dependency; all serde types are supported natively |
| Runtime model | Templates iterate and read runtime values (serde structs, `serde_json::Value`, maps) — no compile-time template typing. This is the same dynamic model 01 §3.1's killer requirement demands |
| Template model | `{% extends %}` / `{% block %}` / `{% include %}` / `{% macro %}` / `{% call %}` / `{% set %}` / `{% with %}` / `{% for %}` / `{% if %}` / `{% filter %}` / `{% autoescape %}` / `{% raw %}` — at feature parity with Jinja2 (details in §3) |
| Sharing | `Environment` is `Send + Sync` once built; rendering takes `&self`, registration takes `&mut`. `Arc<Environment>` is the standard multi-thread pattern — same shape as 01's `Arc<Tera>` |
| Ecosystem satellites | `minijinja-autoreload` (dev hot reload), `minijinja-embed` (templates compiled into the binary, with **build-time syntax validation**), `minijinja-contrib` (extra filters), `minijinja-cli` (render templates from the shell), online playground |
| Production users | HuggingFace TGI, mistral.rs, PRQL, maturin, cargo-dist, OpenTelemetry Weaver, Cube, qsv |

---

## 2. Why MiniJinja over Tera

The core requirement from 01 §3.1 — **dynamic field rendering** (`{% for field in fields %}` over a runtime `Vec<Field>`) — is satisfied identically by Tera and MiniJinja: both are runtime engines over serde values. Askama remains excluded for the same reason as before (compile-time templates can't iterate a runtime `Vec<Field>`). So the real comparison is Tera vs. MiniJinja:

| Concern | Tera 1 | MiniJinja 2 (chosen) |
| --- | --- | --- |
| Syntax lineage | "Inspired by Jinja2" — documented divergences, filters and semantics differ | Jinja2 itself; COMPATIBILITY.md is the deviation ledger |
| Knowledge transfer | Tera-specific; most engineers have never touched it | Jinja2 — the single most widely known server-side template language on the market (Django, Flask, Ansible, dbt, Airflow, Home Assistant) |
| Editor tooling | A handful of Tera extensions | The entire Jinja2 toolchain: VS Code "Jinja" (+ Django/Ansible variants), PyCharm, Sublime, vim; `.j2` files are recognized everywhere |
| Author alignment | Independent of Jinja2's evolution | Ronacher himself; "stay close to Jinja2" is the project's explicit goal |
| Compile perf (project's own bench, MBP 2021) | 63.6 µs/template | 3.87 µs/template (~16×) |
| Render perf (same bench) | 6.86 µs | 3.74 µs (~1.8×) |
| Per-request context | `Context` only; functions take args | `State` — functions can read the render context (actor, request) without it being threaded through every call site (§6.1) |
| Hot reload | Hand-rolled `notify` watcher (01 §10.4) | `minijinja-autoreload::AutoReloader` — watch a dir, rebuild env, done |
| Template debugging | Test harness only | `minijinja-cli` renders a template with JSON/YAML data from the shell; online playground; errors carry template name + line/column |
| Deployment | Load from disk at boot | `minijinja-embed` compiles built-in templates into the binary *and fails the build on invalid syntax* — the single-binary story from 01 §0 stays intact, user overrides still load from disk |
| Autoescape | On by default for `.html` | Opt-in callback — a footgun to manage explicitly (§5.2) |
| Date/format filters (Tera's `date`, `timestamp`) | Ships them | Core stays dep-free by design; the framework registers its own (§3, §6.1) |

The decision is not "Tera is bad." Tera is a fine engine and 01 §12's template ports almost line-for-line (§8). The decision is that the framework's template language is a *consumer-facing surface* — teams override templates, write custom pages, read generated markup — and Jinja2 is the language the industry already knows. MiniJinja delivers it with the best maintenance pedigree available in Rust, at better performance, with the dev-loop tooling 04 flagged as Rust's weakest point.

---

## 3. Honest gaps — Jinja2 vs. MiniJinja, and who covers them

From COMPATIBILITY.md (v2.23.0). Each gap rated for impact on Twentytoo's built-in templates and the user-facing template surface:

| Jinja2 feature | MiniJinja status | Impact on us | Cover |
| --- | --- | --- | --- |
| `for` / `if` / `extends` / `block` / `call` / `do` / `with` / `set` / `filter` / `autoescape` / `raw` | Feature parity | None — this is the entire structural surface of 01 §12's templates | — |
| `include` with `without context` / `with context` modifiers | Not supported (context always passed) | None — we never need context isolation | — |
| `macro` `varargs` / `kwargs` | Not supported | None — framework templates use explicit macro args | — |
| `continue` / `break` | Behind `loop_controls` feature | Minor — enable it at build time | `minijinja = { features = ["loop_controls"] }` |
| Python methods (`x.items()`) | None; `|items` filter instead | None — Jinja2 itself now recommends filters | — |
| `%` string formatting (`"x %s" % v`) | Not supported | None — use the `format` filter | — |
| String filters `xmlattr`, `urlize` | Missing | None — framework doesn't need them | — |
| Filters with `attribute` argument | Some don't support it | Low — framework sorts in Rust, not in templates | — |
| Date/time formatting (Tera's `date` role) | Not in core (deliberately dep-free) | Medium — every list view renders `created_at` | Framework registers `format_datetime` (§6.1) |
| HTML escaping | Html + extensible to other forms | Medium — must be configured, not assumed | §5.2 |
| Undefined variable behavior | Configurable (`Lenient` default, `Strict` available) | Medium — a typo'd variable silently renders empty by default | §6.4 |
| Unicode identifiers | Off by default (`unicode` feature) | None | — |

None of the gaps touch the killer requirement (dynamic iteration over runtime values), and the ones that matter have a one-line cover in the framework. The two that deserve real design attention — autoescape configuration and undefined-variable strictness — are handled in §5.2 and §6.4, because both touch the "XSS-safe templates" and "misconfiguration caught early" promises from 00 §2.

---

## 4. Delta from 01-rust-implementation.md

Section-by-section changes; everything not listed is unchanged.

| 01 § | Was | Now |
| --- | --- | --- |
| §1.2 crate table | `Template rendering (Tera)`, dep `tera` | `Template rendering (Jinja2 via minijinja)`, deps `minijinja` + `minijinja-embed` |
| §3.1 | Decision: Tera; table Tera vs. Askama | Replaced by this doc §2 (MiniJinja vs. Tera vs. Askama) |
| §3.2 | Template inventory, `*.html.tera` names | Same inventory, `*.html.j2` names (§5.1) |
| §3.3 | Custom functions `can(actor, …)`, `flag(actor, …)`, `format_field`, `metric_value` | Same functions, Jinja2 registration; `actor` moves into `State`, so templates call `can("stores.view")` (§6.1) |
| §4.2 | `tera::Context::new()` + `ctx.insert(...)` | `minijinja::context!` + `env.render(name, ctx)` (§7) |
| §4.4 | `templates: Arc<Tera>` | `templates: Arc<TemplateEngine>` — a framework wrapper owning `Environment<'static>` |
| §6.3 | `PageContext.templates: Arc<Tera>` | `Arc<TemplateEngine>`; custom pages render with the same env |
| §8.1 | `tera 1` | `minijinja 2`, `minijinja-embed 1` (regular **and** build dep); dev-only `minijinja-autoreload`, `minijinja-cli` (§11) |
| §8.2 | Avoid: `askama` | Keep `askama`; add Tera to the same list — neither is a dependency |
| §10.3 | `AppError::Template(TeraError)` | `AppError::Template(minijinja::Error)` — structured `ErrorKind` (SyntaxError, TemplateNotFound, UnknownFunction, UndefinedError, …) plus template name and line/column, logged via `tracing` |
| §10.4 | Hand-rolled `notify` watcher for Tera reload | `AutoReloader` from `minijinja-autoreload`, dev-only (§6.3) |
| §12 | `resource/list.html.tera` snippet | Ported to `resource/list.html.j2` (§8) — near line-for-line |

---

## 5. Template conventions

### 5.1 Naming

- HTML templates: `resource/list.html.j2`, `resource/detail.html.j2`, `resource/form.html.j2`, `layout/base.html.j2`, `partials/pagination.html.j2`, `dashboard/home.html.j2`, `audit/list.html.j2`, `users/login.html.j2`, `flags/list.html.j2`
- Plain-text templates (email bodies per 02 §3.3, notification text, export row templates per 02 §9): `mail/export_ready.txt.j2`, `mail/invite.txt.j2`, `export/orders.csv.j2`

`.j2` is the extension the Jinja2 editor ecosystem recognizes (Ansible/Flask convention), and it makes autoescape classification unambiguous (§5.2). Template names are loader-relative paths: `{% extends "layout/base.html.j2" %}`.

### 5.2 Autoescape policy — a framework rule, not a default

MiniJinja does **not** autoescape by default (Tera did). The framework sets it explicitly at env build:

```rust
env.set_auto_escape_callback(|name| {
    if name.ends_with(".html.j2") {
        minijinja::AutoEscape::Html
    } else {
        minijinja::AutoEscape::None
    }
});
```

Rule: **any template that emits HTML is autoescaped; any template that emits another format (email text, CSV) is not.** A template that needs raw HTML in an autoescaped file uses `|safe` explicitly — and framework functions that render HTML (`format_field`, `metric_value`) return `Value::from_safe_string(...)` with their own internal escaping (every dynamic fragment escaped, structure framework-owned). This preserves 00's XSS-by-default posture with one reviewable line in one place.

### 5.3 Resolution and user overrides

01's requirement stands: a user drops a template in their directory and it wins over the framework's built-in. MiniJinja resolves a name by checking *eagerly registered* templates first, then the loader — so the order of registration is the override mechanism:

```rust
// build.rs — built-ins compiled into the binary; INVALID SYNTAX FAILS THE BUILD
minijinja_embed::embed_templates!("templates", &[".j2"]);

// env build — three steps, in order:
let mut env = Environment::new();
minijinja_embed::load_templates!(&mut env);        // 1. built-ins, eagerly registered
for (name, src) in read_user_templates(override_dir) {
    env.add_template(name, src)?;                  // 2. user overrides — same name REPLACES
}
env.set_loader(path_loader(override_dir));         // 3. fallback for user-only templates
```

- Step 1 is what keeps the framework's templates inside the single static binary (01 §0's deployment story). Bonus: `embed_templates!` panics at **build** time on invalid syntax — the framework's own templates are compile-checked, a stronger form of 01 §3.1's "correct by construction."
- Step 2 replaces by name (`add_template` overwrites), so user overrides win; step 3 catches templates that exist only in the user's directory (custom-page templates).
- `read_user_templates` is a startup walk of the override dir; `path_loader` handles the lazy remainder. Both are dev/prod identical; reload is the only thing that differs (§6.3).

### 5.4 Sharing

`Environment<'static>` is `Send + Sync`; registration methods (`add_function`, `add_filter`, `add_test`, `set_loader`, `set_auto_escape_callback`) take `&mut` at build time; `render` takes `&self`. The framework builds the env once at startup, wraps it in `Arc`, and every handler and custom page renders through it — identical shape to 01 §4.4's `Arc<Tera>`, wrapped in a `TemplateEngine` struct so the rest of the framework never names a template crate directly.

---

## 6. Framework-registered functions, filters, tests

Registered once at env build in `twentytoo`'s template module:

| Kind | Name | Signature | Purpose |
| --- | --- | --- | --- |
| function | `can` | `can(permission) -> bool` | RBAC check; reads `actor` from `State`, checks `actor.permissions` |
| function | `flag` | `flag("name") -> bool` | Flag resolution; `State` gives the actor for targeting; a non-existent flag → false |
| function | `format_field` | `format_field(value, field_kind) -> SafeString` | Field renderer by `FieldKind` (badge pill, relation link, image tag, …); returns `Value::from_safe_string` with internal escaping |
| function | `format_filter` | `format_filter(filter) -> SafeString` | Filter control widget for the list sidebar |
| function | `metric_value` | `metric_value(key) -> SafeString` | Rendered metric card HTML |
| filter | `format_datetime` | `value|format_datetime(fmt) -> String` | chrono-backed date rendering (covers Tera's `date` role); accepts ISO-8601 strings or serde datetimes |
| filter | `currency` | `value|currency -> String` | Money formatting for `FieldKind::Currency` |
| test | `permission` | `"stores.view" is permission` | Optional sugar over `can()`; cut it if the first real template doesn't use it |

Two design points:

1. **`actor` lives in the render context; functions read it via `State`.** 01's `can(actor, "stores.view")` becomes `can("stores.view")` — every template call site loses the `actor` argument, and custom-page authors can't forget to pass it (a forgotten actor would have made `can()` check against nothing; with `State`, the actor is *always* present or the render fails). This is the `State` API's concrete payoff over Tera.
2. **Safe-string contract:** HTML-returning functions escape every dynamic fragment they embed (values, labels, attribute contents). Everything else in the template is autoescaped by the environment. One reviewer-facing rule: "safe-string-returning functions escape internally; templates escape by default."

---

## 7. Handler integration (01 §4.2 ported)

```rust
let html = state.templates.render(
    "resource/list.html.j2",
    minijinja::context! {
        resource => &resource_view,    // ResourceViewModel
        items => &result.items,        // Vec<E> — anything Serialize
        pagination => &result,         // Page<E>
        actor => &actor,               // State carrier for can()/flag()
    },
)?;                                    // From<minijinja::Error> → AppError::Template
Ok(Html(html))
```

`context!` accepts anything `Serialize`; entities enter as serde values, which is exactly the "view layer touches entities only as serialized JSON" rule from 03 §3.1 — `item[col]` in the template works identically whether `E` is a typed struct or `serde_json::Value`. The adapter layer is untouched by this change.

---

## 8. The list template, in Jinja2 (01 §12 ported)

```jinja
{# resource/list.html.j2 #}
{% extends "layout/base.html.j2" %}

{% block content %}
<div class="resource-header">
    <h1>{{ resource.label }}</h1>
    {% if can(resource.key ~ ".create") and flag(resource.flag) %}
        <a href="/{{ resource.key }}/new" class="btn btn-primary">New {{ resource.label }}</a>
    {% endif %}
</div>

{# Search + filter bar #}
<form hx-get="/{{ resource.key }}" hx-target="#resource-table" hx-trigger="submit">
    <input type="search" name="search" placeholder="Search {{ resource.label }}..."
           hx-get="/{{ resource.key }}" hx-target="#resource-table"
           hx-trigger="keyup changed delay:300ms" />
    {% for filter in resource.filters %}
        {{ format_filter(filter) }}
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
                        <td>{{ format_field(item[col], resource.fields[col].kind) }}</td>
                    {% endfor %}
                    <td>
                        {% for action in resource.actions %}
                            {% if can(action.policy) and flag(action.flag) %}
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

    {% include "partials/pagination.html.j2" %}
</div>
{% endblock %}
```

Notes:

- This is 01 §12 with syntax-level differences only: the `{% extends %}` target (extension change), `can("...")` (State-based actor, §6.1), and no `|safe` on `format_field` (safe-string return instead of Tera's escaped-then-safe-marked HTML).
- `~` string concat is Jinja2 syntax — no change from 01's template.
- htmx attributes, RBAC gating, flag gating, the single-swap-target table — all unchanged. The template is still framework-owned, still overrideable per §5.3.

---

## 9. Beyond HTML: emails, exports, notifications (02 touchpoints)

- **Email (02 §3.3):** `mail/*.txt.j2` rendered by the same env (autoescape off — text, not HTML). HTML+text mail pairs later are `.html.j2`/`.txt.j2` siblings; nothing new to build.
- **Exports (02 §9):** export row templates (`export/*.csv.j2`) render through minijinja. Rendering is synchronous (`render` → `String`), so the async export job does its I/O around the template call — same shape as Tera.
- **Notification strings (02 §3.3):** the `template: "A new store '{{ record.name }}' was created"` strings are mini templates. `Environment::render_str` evaluates them with a small context — no second engine, and the strings are Jinja2, matching everything else.
- **SSE partials (02 §2.4):** unchanged — a partial is just another name in the env (`partials/table.html.j2`).

---

## 10. Dev loop and debugging (the answer to 04 §2's "weakest point" call)

| Tool | Role |
| --- | --- |
| `minijinja-autoreload` | `AutoReloader` watches the template directory and rebuilds the environment on change — re-running §5.3's three steps. Replaces 01 §10.4's hand-rolled watcher. Dev-only dependency. |
| `minijinja-cli` | Render any framework template with fixture JSON/YAML from the shell — iterate on markup without a server round-trip. Also usable in CI to snapshot rendered output. |
| Playground | `mitsuhiko.github.io/minijinja-playground/` — syntax-check a snippet before pasting it into the repo. |
| Build-time validation | `minijinja-embed` fails the build on invalid syntax in built-in templates — framework template typos are CI errors, not 500s. |
| Strict undefined (dev) | Dev profile sets `UndefinedBehavior::Strict` so a typo'd variable name fails loudly instead of rendering empty; prod keeps the default `Lenient` so an optional missing field degrades gracefully. |
| Errors | `minijinja::Error` carries `ErrorKind`, template name, and line/column; the `AppError::Template` arm logs it with `tracing`. |

---

## 11. Dependency table (01 §8 delta)

| Dependency | Version | Purpose | Note |
| --- | --- | --- | --- |
| `minijinja` | 2 (2.23.0 at time of writing) | Jinja2 template engine | `features = ["loop_controls"]` only if a template needs `{% break %}` / `{% continue %}` |
| `minijinja-embed` | 1 | Built-in templates compiled into the binary; build-time syntax validation | Regular **and** `[build-dependencies]` (it's invoked from `build.rs`) |
| `minijinja-autoreload` | 1 | Dev-only template hot reload | dev-dependency or feature-gated |
| `minijinja-cli` | — | Dev tool, not a dependency | shell utility; optional in CI |

Removed from 01 §8.1: `tera 1`. Added to the §8.2 avoid list: Tera (same category as Askama — not a dependency; the engine is MiniJinja).

---

## 12. Verification

- CI renders every built-in template against fixture data (unchanged from 01 §3.1's mitigation — the requirement survives the engine swap).
- Boot test: after the §5.3 env build, `get_template` every name the framework references (layouts, partials, includes) — catches a missing or mistyped template name before first request. `embed_templates!` already covers syntax; this covers reference integrity.
- Because the syntax is Jinja2, the same templates can be cross-rendered by Python's `jinja2` (or `minijinja-py`) in a parity test — a check that would be impossible with Tera. Optional; don't build the pipeline until a template actually misbehaves.
- Dev-loop smoke: `AutoReloader` picks up an edit to a user override without restart (manual, once).

---

## 13. Open questions

1. **Extension scheme.** `.html.j2` / `.txt.j2` (this doc) vs. plain `.html` with the editor told to treat it as Jinja. `.j2` wins on editor recognition and autoescape clarity; plain `.html` wins on "no new extension." Deferred to the first real user override.
2. **Strict undefined in dev** — default-on or opt-in? Leaning: default-on for the framework's own templates (they're tested), opt-in for user overrides (a partial custom template may legitimately reference absent fields).
3. **`minijinja-contrib`** — adopt wholesale (`json`, `urlencode`, …) or register filters individually as needed? Leaning: individually; contrib is small but core templates don't need most of it.
4. **The `permission` test** — sugar over `can()`; cut it if unused in the first real template.
5. **`~` concat** — identical in Tera and Jinja2; listed only so nobody "fixes" it during review.

---

## 14. What this does not change

00's rendering model, 02's feature set, 03's adapter/query/capability design, 01's traits/builders/router/middleware — all untouched. The engine is a swap behind `TemplateEngine`, and every framework API surface that mentions templates (`PageContext`, `AppError::Template`, `with_template_dir`) keeps its shape. Per 04 §9's own logic: the 03 capability model remains the portability layer; the template engine is now the least interesting thing to port, because the templates themselves are already standard Jinja2.
