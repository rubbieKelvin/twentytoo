//! The MiniJinja environment the framework renders through (`00` §8).
//!
//! Built-ins compile into the binary with build-time syntax validation
//! (`build.rs`); user overrides replace them by name and a path loader
//! catches user-only templates (`00` §8.5). Autoescape is a framework
//! rule, not a default: `.html.j2` escapes by default, everything else
//! renders raw; safe-string-returning functions escape internally
//! (`00` §8.3).

use std::path::Path;

use chrono::{DateTime, FixedOffset};
use minijinja::value::ViaDeserialize;
use minijinja::{AutoEscape, Environment, State, Value};
use serde::Deserialize;
use twentytoo_core::Actor;

use crate::application::dto::{FieldView, FilterView, KindView};
use crate::shared::utils::{escape_html, format_money};

/// Framework templates referenced by handlers — the boot check (`00` §8.5)
/// verifies each of these resolves after the env build.
pub const BUILTIN_TEMPLATES: &[&str] = &[
    "layout/base.html.j2",
    "layout/auth.html.j2",
    "dashboard/home.html.j2",
    "resource/list.html.j2",
    "resource/detail.html.j2",
    "resource/form.html.j2",
    "partials/list.html.j2",
    "partials/pagination.html.j2",
    "auth/email.html.j2",
    "auth/code.html.j2",
    "auth/password.html.j2",
    "users/list.html.j2",
    "users/form.html.j2",
];

/// The closed inline-SVG icon set (`01-ui-kit` §7.13). `Resource::icon()`
/// values and template `icon(name)` calls must resolve here; the boot check
/// (`container.rs`) validates every registered resource icon against it.
pub const ICON_NAMES: &[&str] = &[
    "alert",
    "calendar",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "cube",
    "dot",
    "edit",
    "external",
    "file",
    "filter",
    "home",
    "inbox",
    "logout",
    "more-horizontal",
    "plus",
    "search",
    "settings",
    "sort",
    "spinner",
    "trash",
    "users",
    "x",
];

/// The inner markup for one icon name: 24x24 viewBox, stroke 1.5,
/// `currentColor` — colored purely by CSS.
fn icon_paths(name: &str) -> &'static str {
    return match name {
        "alert" => {
            "<path d=\"M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z\"/><path d=\"M12 9v4\"/><path d=\"M12 17h.01\"/>"
        }
        "calendar" => {
            "<rect x=\"3\" y=\"4\" width=\"18\" height=\"18\" rx=\"2\"/><path d=\"M16 2v4M8 2v4M3 10h18\"/>"
        }
        "check" => "<path d=\"M20 6L9 17l-5-5\"/>",
        "chevron-down" => "<path d=\"M6 9l6 6 6-6\"/>",
        "chevron-left" => "<path d=\"M15 18l-6-6 6-6\"/>",
        "chevron-right" => "<path d=\"M9 18l6-6-6-6\"/>",
        "chevron-up" => "<path d=\"M18 15l-6-6-6 6\"/>",
        "cube" => {
            "<path d=\"M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z\"/><path d=\"M3.27 6.96L12 12.01l8.73-5.05\"/><path d=\"M12 22.08V12\"/>"
        }
        "dot" => "<circle cx=\"12\" cy=\"12\" r=\"5\" fill=\"currentColor\" stroke=\"none\"/>",
        "edit" => {
            "<path d=\"M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7\"/><path d=\"M18.5 2.5a2.12 2.12 0 013 3L12 15l-4 1 1-4 9.5-9.5z\"/>"
        }
        "external" => {
            "<path d=\"M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6\"/><path d=\"M15 3h6v6\"/><path d=\"M10 14L21 3\"/>"
        }
        "file" => {
            "<path d=\"M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z\"/><path d=\"M14 2v6h6\"/>"
        }
        "filter" => "<path d=\"M22 3H2l8 9.46V19l4 2v-8.54L22 3z\"/>",
        "home" => {
            "<path d=\"M3 10.5L12 3l9 7.5\"/><path d=\"M5 9.5V21h14V9.5\"/><path d=\"M9.5 21v-6h5v6\"/>"
        }
        "inbox" => {
            "<path d=\"M22 12h-6l-2 3h-4l-2-3H2\"/><path d=\"M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z\"/>"
        }
        "logout" => {
            "<path d=\"M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4\"/><path d=\"M16 17l5-5-5-5\"/><path d=\"M21 12H9\"/>"
        }
        "more-horizontal" => {
            "<circle cx=\"5\" cy=\"12\" r=\"1\"/><circle cx=\"12\" cy=\"12\" r=\"1\"/><circle cx=\"19\" cy=\"12\" r=\"1\"/>"
        }
        "plus" => "<path d=\"M12 5v14M5 12h14\"/>",
        "search" => "<circle cx=\"11\" cy=\"11\" r=\"7\"/><path d=\"M20 20l-3.5-3.5\"/>",
        "settings" => {
            "<path d=\"M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3\"/><path d=\"M14 2v4M8 10v4M16 18v4\"/>"
        }
        "sort" => "<path d=\"M8 9l4-4 4 4\"/><path d=\"M8 15l4 4 4-4\"/>",
        "spinner" => "<circle cx=\"12\" cy=\"12\" r=\"9\" stroke-dasharray=\"30 30\"/>",
        "trash" => {
            "<path d=\"M3 6h18\"/><path d=\"M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6\"/><path d=\"M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2\"/><path d=\"M10 11v6M14 11v6\"/>"
        }
        "users" => {
            "<path d=\"M16 21v-2a4 4 0 00-4-4H6a4 4 0 00-4 4v2\"/><circle cx=\"9\" cy=\"7\" r=\"4\"/><path d=\"M22 21v-2a4 4 0 00-3-3.87\"/><path d=\"M16 3.13a4 4 0 010 7.75\"/>"
        }
        "x" => "<path d=\"M18 6L6 18M6 6l12 12\"/>",
        // Unknown names render the neutral dot fallback (01 §7.13).
        _ => "<circle cx=\"12\" cy=\"12\" r=\"5\" fill=\"currentColor\" stroke=\"none\"/>",
    };
}

/// One inline SVG icon as a string (used by the `icon` function and by
/// field formatting).
///
/// The markup rides Tabler's `.icon` sizing: the inline
/// `--tblr-icon-size` override pins the exact pixel size, and the
/// `width`/`height` attributes are the no-CSS fallback (`01-ui-kit` §7.10).
fn icon_svg(name: &str, size: usize) -> String {
    return format!(
        "<svg class=\"icon\" style=\"--tblr-icon-size:{size}px\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\">{}</svg>",
        icon_paths(name)
    );
}

/// Template function: `icon("check")` / `icon("check", 20)` — one inline
/// SVG from the closed icon set (`01-ui-kit` §7.13). Safe string; the name
/// is an internal lookup, never user markup.
fn icon(name: String, size: Option<u64>) -> Value {
    let size = size.unwrap_or(16) as usize;
    return Value::from_safe_string(icon_svg(&name, size));
}

/// The deterministic avatar hue class for a name (`01-ui-kit` §7.3): the
/// same string always yields the same color, server- and client-free.
/// The classes are Tabler's soft background hues (`bg-*-lt`), so the
/// avatar renders as a tinted circle with initials.
fn avatar_class(name: &str) -> String {
    let mut hash: u32 = 5381;
    for b in name.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    const HUES: [&str; 9] = [
        "bg-red-lt",
        "bg-orange-lt",
        "bg-green-lt",
        "bg-teal-lt",
        "bg-blue-lt",
        "bg-indigo-lt",
        "bg-purple-lt",
        "bg-yellow-lt",
        "bg-secondary-lt",
    ];
    return HUES[(hash % 9) as usize].to_string();
}

/// Template filter: `email|avatar_hue` — the avatar hue class for a name.
fn avatar_hue(value: &Value) -> String {
    return avatar_class(&value.to_string());
}

/// Template filter: `name|initial` — the first character, uppercased, for
/// avatar initials. Plain string; the template escapes it.
fn initial(value: &Value) -> String {
    return value
        .to_string()
        .chars()
        .next()
        .map(|c| return c.to_uppercase().to_string())
        .unwrap_or_default();
}

/// The built environment, wrapped so the rest of the framework never names
/// a template crate directly.
pub struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Build the environment: embedded built-ins, then user overrides, then
    /// a path loader for user-only templates (`00` §8.5).
    pub fn new(override_dir: Option<&Path>) -> Result<Self, minijinja::Error> {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|name| {
            if name.ends_with(".html.j2") {
                return AutoEscape::Html;
            }
            return AutoEscape::None;
        });

        // 1. Built-ins, eagerly registered (compiled into the binary).
        minijinja_embed::load_templates!(&mut env);

        // 2 + 3. User overrides (same name REPLACES) and user-only
        // templates, loaded from the override dir when configured.
        if let Some(dir) = override_dir {
            let entries = std::fs::read_dir(dir).map_err(|e| {
                return minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("template dir {:?} unreadable: {e}", dir.display()),
                );
            })?;
            for entry in entries {
                let path = match entry {
                    Ok(e) => e.path(),
                    Err(_) => continue,
                };
                if path.extension().is_some_and(|ext| return ext == "j2") {
                    let name = path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let src = std::fs::read_to_string(&path).map_err(|e| {
                        return minijinja::Error::new(
                            minijinja::ErrorKind::InvalidOperation,
                            format!("template {name} unreadable: {e}"),
                        );
                    })?;
                    env.add_template_owned(name, src)?;
                }
            }
            env.set_loader(minijinja::path_loader(dir));
        }

        // Framework functions and filters (`00` §8.4). `flag` and
        // `metric_value` register with their slices.
        env.add_function("can", can);
        env.add_function("icon", icon);
        env.add_function("format_field", format_field);
        env.add_function("format_filter", format_filter);
        env.add_function("form_control", form_control);
        env.add_filter("format_datetime", format_datetime);
        env.add_filter("currency", currency);
        env.add_filter("avatar_hue", avatar_hue);
        env.add_filter("initial", initial);
        // Boot check (`00` §8.5): every referenced name must resolve.
        for name in BUILTIN_TEMPLATES {
            env.get_template(name)?;
        }

        return Ok(Self { env });
    }

    /// Render `name` with `ctx` (anything `Serialize`).
    pub fn render<S: serde::Serialize>(
        &self,
        name: &str,
        ctx: &S,
    ) -> Result<String, minijinja::Error> {
        let template = self.env.get_template(name)?;
        return template.render(ctx);
    }
}

/// Template function: `can("stores.create")` — RBAC check over the actor in
/// the render context, read via `State` (`00` §8.4). No actor in context →
/// deny.
fn can(state: &State, permission: String) -> bool {
    let Some(actor) = state.lookup("actor") else {
        return false;
    };
    let Ok(actor) = Actor::deserialize(&actor) else {
        return false;
    };
    return actor.can(&permission);
}

/// Template function: `format_field(value, kind)` — render one cell.
/// Returns a safe string; every dynamic fragment is escaped internally
/// (`00` §8.3).
fn format_field(value: Value, kind: ViaDeserialize<KindView>) -> Value {
    let kind = &*kind;
    let text = match kind.tag.as_str() {
        // Numbers and money render as plain text; the list template
        // right-aligns the cell with `text-end`.
        "currency" => match as_f64(&value) {
            Some(n) => escape_html(&format_money(n)),
            None => escape_html(&value.to_string()),
        },
        "number" => escape_html(&value.to_string()),
        "datetime" => escape_html(&format_datetime(&value, "%Y-%m-%d %H:%M".to_string())),
        "date" => escape_html(&format_datetime(&value, "%Y-%m-%d".to_string())),
        "boolean" => {
            if value.is_true() {
                format!(
                    "<span class=\"d-inline-flex align-items-center gap-1 text-success\">{}Yes</span>",
                    icon_svg("check", 14)
                )
            } else {
                format!(
                    "<span class=\"d-inline-flex align-items-center gap-1 text-danger\">{}No</span>",
                    icon_svg("x", 14)
                )
            }
        }
        "badge" => badge_pill(kind, value.as_str()),
        "multiselect" => {
            let mut pills: Vec<String> = value
                .try_iter()
                .map(|iter| return iter.map(|v| return badge_pill(kind, v.as_str())).collect())
                .unwrap_or_default();
            if pills.is_empty() {
                pills.push(badge_pill(kind, value.as_str()));
            }
            pills.join("")
        }
        "select" => label_for(kind, value.as_str()),
        "relation" => relation_link(kind, &value.to_string()),
        // text, textarea, richtext, email, json, file, image, computed —
        // plain escaped text (computed values were materialized into the
        // row by the handler).
        _ => escape_html(&value.to_string()),
    };
    return Value::from_safe_string(text);
}

/// One `Badge`/`MultiSelect` value as a soft pill (`01-ui-kit` §7.2). The
/// semantic class follows the option's position in the declaration —
/// config order is semantic order; unknown values fall back to neutral.
/// Classes are Tabler's soft badge variants (`bg-*-lt`).
fn badge_pill(kind: &KindView, value: Option<&str>) -> String {
    let label = label_for(kind, value);
    let Some(value) = value else {
        return format!("<span class=\"badge bg-secondary-lt\">{label}</span>");
    };
    let Some(index) = kind.options.iter().position(|o| return o.value == value) else {
        return format!("<span class=\"badge bg-secondary-lt\">{label}</span>");
    };
    const SEMANTIC: [&str; 5] = ["primary", "success", "warning", "danger", "info"];
    let cls = SEMANTIC[index % SEMANTIC.len()];
    return format!("<span class=\"badge bg-{cls}-lt\">{label}</span>");
}

/// A `Relation` value as avatar + id link (`01-ui-kit` §7.3/§11.4). The
/// avatar hue is deterministic from the id; the id is the display value —
/// entities travel as JSON, so the related record's display field is not
/// available at render time.
fn relation_link(kind: &KindView, id: &str) -> String {
    let id_esc = escape_html(id);
    let initials = escape_html(&id.chars().take(2).collect::<String>().to_uppercase());
    let avatar = format!(
        "<span class=\"avatar avatar-sm {}\">{initials}</span>",
        avatar_class(id)
    );
    let resource_key = escape_html(kind.relation.as_deref().unwrap_or(""));
    let row = format!("{avatar}<span>{id_esc}</span>");
    if resource_key.is_empty() {
        return format!("<span class=\"d-inline-flex align-items-center gap-2\">{row}</span>");
    }
    return format!(
        "<a class=\"d-inline-flex align-items-center gap-2 text-reset text-decoration-none\" href=\"/{resource_key}/{id_esc}\">{row}</a>"
    );
}

/// The label for a value within a select-like kind; unknown values render
/// as themselves (escaped by the caller).
fn label_for(kind: &KindView, value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    for option in &kind.options {
        if option.value == value {
            return escape_html(&option.label);
        }
    }
    return escape_html(value);
}

/// Template function: `format_filter(filter)` — the toolbar control for one
/// filter. Safe string; values and labels escaped internally. Controls are
/// Tabler's small `form-select`/`form-control` variants; the toolbar form
/// submits them on Apply/Enter.
fn format_filter(filter: ViaDeserialize<FilterView>) -> Value {
    let f = &*filter;
    let name = escape_html(&f.name);
    let label = escape_html(&f.label);
    let current = escape_html(f.current.as_deref().unwrap_or(""));
    let html = match f.op.as_str() {
        "eq" | "in" | "notin"
            if matches!(f.kind.tag.as_str(), "select" | "badge" | "multiselect") =>
        {
            let mut out = format!(
                "<div class=\"d-inline-flex align-items-center gap-2\"><span class=\"form-label mb-0\">{label}</span><select class=\"form-select form-select-sm\" name=\"{name}\">"
            );
            out.push_str(&format!("<option value=\"\">All {label}</option>"));
            for option in &f.kind.options {
                let selected = if Some(option.value.as_str()) == f.current.as_deref() {
                    " selected"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<option value=\"{}\"{}>{}</option>",
                    escape_html(&option.value),
                    selected,
                    escape_html(&option.label)
                ));
            }
            out.push_str("</select></div>");
            out
        }
        "gt" | "gte" | "lt" | "lte" if matches!(f.kind.tag.as_str(), "number" | "currency") => {
            format!(
                "<div class=\"d-inline-flex align-items-center gap-2\"><span class=\"form-label mb-0\">{label}</span><input class=\"form-control form-control-sm\" type=\"number\" name=\"{name}\" value=\"{current}\" placeholder=\"{label}\"></div>"
            )
        }
        "gt" | "gte" | "lt" | "lte" if matches!(f.kind.tag.as_str(), "date") => {
            format!(
                "<div class=\"d-inline-flex align-items-center gap-2\"><span class=\"form-label mb-0\">{label}</span><input class=\"form-control form-control-sm\" type=\"date\" name=\"{name}\" value=\"{current}\"></div>"
            )
        }
        _ => {
            format!(
                "<div class=\"d-inline-flex align-items-center gap-2\"><span class=\"form-label mb-0\">{label}</span><input class=\"form-control form-control-sm\" type=\"text\" name=\"{name}\" value=\"{current}\" placeholder=\"{label}\"></div>"
            )
        }
    };
    return Value::from_safe_string(html);
}

/// The current value as a string for `value="…"` attributes: strings
/// directly, numbers/bools via `to_string`, missing → empty.
fn value_string(value: &Value) -> String {
    if value.is_undefined() {
        return String::new();
    }
    return value
        .as_str()
        .map(|s| return s.to_string())
        .unwrap_or_else(|| return value.to_string());
}

/// Template function: `form_control(field, values, errors)` — one form
/// widget. Safe string; current values escaped internally. `errors` is
/// the field-error map; a field with an error gets Bootstrap's
/// `is-invalid` class so the template's `invalid-feedback` block shows.
fn form_control(field: ViaDeserialize<FieldView>, values: Value, errors: Value) -> Value {
    let f = &*field;
    let name = escape_html(&f.name);
    let id = format!("f-{name}");
    let current = values.get_attr(&f.name).unwrap_or_default();
    // `get_attr` answers `Ok(UNDEFINED)` for a missing key: an error
    // exists only when a defined value is returned.
    let invalid = if errors
        .get_attr(&f.name)
        .map(|v| return !v.is_undefined())
        .unwrap_or(false)
    {
        " is-invalid"
    } else {
        ""
    };
    let html = match f.kind.tag.as_str() {
        "boolean" => {
            let checked = if current.is_true() { " checked" } else { "" };
            format!(
                "<label class=\"form-check form-switch\"><input class=\"form-check-input\" type=\"checkbox\" id=\"{id}\" name=\"{name}\"{checked}{invalid}></label>"
            )
        }
        "textarea" | "richtext" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<textarea class=\"form-control{invalid}\" id=\"{id}\" name=\"{name}\" rows=\"5\">{v}</textarea>"
            )
        }
        "select" | "badge" => {
            let mut out =
                format!("<select class=\"form-select{invalid}\" id=\"{id}\" name=\"{name}\">");
            out.push_str("<option value=\"\"></option>");
            let current_s = current.as_str();
            for option in &f.kind.options {
                let selected = if current_s == Some(option.value.as_str()) {
                    " selected"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<option value=\"{}\"{}>{}</option>",
                    escape_html(&option.value),
                    selected,
                    escape_html(&option.label)
                ));
            }
            out.push_str("</select>");
            out
        }
        "multiselect" => {
            let selected: Vec<String> = current
                .try_iter()
                .map(|iter| return iter.map(|v| return v.to_string()).collect())
                .unwrap_or_default();
            let mut out = format!(
                "<select class=\"form-select{invalid}\" id=\"{id}\" name=\"{name}\" multiple size=\"4\">"
            );
            for option in &f.kind.options {
                let chosen = if selected.iter().any(|s| return s == &option.value) {
                    " selected"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<option value=\"{}\"{}>{}</option>",
                    escape_html(&option.value),
                    chosen,
                    escape_html(&option.label)
                ));
            }
            out.push_str("</select>");
            out
        }
        "number" | "currency" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input class=\"form-control{invalid}\" type=\"number\" step=\"any\" id=\"{id}\" name=\"{name}\" value=\"{v}\">"
            )
        }
        "date" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input class=\"form-control{invalid}\" type=\"date\" id=\"{id}\" name=\"{name}\" value=\"{v}\">"
            )
        }
        "datetime" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input class=\"form-control{invalid}\" type=\"text\" id=\"{id}\" name=\"{name}\" value=\"{v}\" placeholder=\"2026-08-13T10:30:00Z\">"
            )
        }
        "email" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input class=\"form-control{invalid}\" type=\"email\" id=\"{id}\" name=\"{name}\" value=\"{v}\">"
            )
        }
        // text, json, relation, computed — plain text input; file/image
        // kinds are excluded from forms by the view model.
        _ => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input class=\"form-control{invalid}\" type=\"text\" id=\"{id}\" name=\"{name}\" value=\"{v}\">"
            )
        }
    };
    return Value::from_safe_string(html);
}

/// Template filter: `value|format_datetime(fmt)` — chrono-backed date
/// rendering (`00` §8.4). Accepts RFC 3339 strings; unparseable input passes
/// through unchanged.
fn format_datetime(value: &Value, fmt: String) -> String {
    let Some(s) = value.as_str() else {
        return value.to_string();
    };
    match DateTime::<FixedOffset>::parse_from_rfc3339(s) {
        Ok(dt) => return dt.format(&fmt).to_string(),
        Err(_) => return s.to_string(),
    }
}

/// Template filter: `value|currency` — money formatting.
fn currency(value: Value) -> String {
    let Some(n) = as_f64(&value) else {
        return value.to_string();
    };
    return format_money(n);
}

/// A value as f64: integers directly, floats via serde extraction.
fn as_f64(value: &Value) -> Option<f64> {
    if let Some(i) = value.as_i64() {
        return Some(i as f64);
    }
    return f64::deserialize(value).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `00` §8.5 boot test: every referenced template resolves and
    /// renders against fixture data — a missing or mistyped name fails
    /// here, before any request.
    #[test]
    fn boot_templates_resolve_and_render() {
        use crate::application::dto::{PagerView, ResourceView};
        use crate::presentation::registry::NavItem;
        use std::collections::HashMap;
        use std::sync::Arc;
        use twentytoo_core::{InMemoryAdapter, Policy, SortField, field};

        struct AllowAll;

        impl<E> Policy<E> for AllowAll {
            fn can_view_any(&self, _actor: &Actor) -> bool {
                return true;
            }

            fn can_create(&self, _actor: &Actor) -> bool {
                return true;
            }

            fn can_update(&self, _actor: &Actor, _record: &E) -> bool {
                return true;
            }

            fn can_delete(&self, _actor: &Actor, _record: &E) -> bool {
                return true;
            }
        }

        struct WidgetResource {
            adapter: Arc<InMemoryAdapter<serde_json::Value>>,
        }

        impl twentytoo_core::Resource for WidgetResource {
            type Entity = serde_json::Value;

            fn key(&self) -> &'static str {
                return "widgets";
            }

            fn label(&self) -> &'static str {
                return "Widgets";
            }

            fn fields(&self) -> Vec<twentytoo_core::Field<Self::Entity>> {
                return twentytoo_core::fields![
                    field!("id", "Id", Text),
                    field!("name", "Name", Text, list: true, detail: true, form: true, required: true, sortable: true),
                    field!("status", "Status", Badge { options: &[("active", "Active")] }, list: true, detail: true, form: true),
                    field!("created_at", "Created", DateTime, list: true, detail: true),
                ];
            }

            fn list_columns(&self) -> Vec<&'static str> {
                return vec!["name", "status", "created_at"];
            }

            fn default_sort(&self) -> Vec<SortField> {
                return vec![SortField::asc("name")];
            }

            fn search_fields(&self) -> Vec<&'static str> {
                return vec!["name"];
            }

            fn policy(&self) -> &dyn Policy<Self::Entity> {
                return &AllowAll;
            }

            fn adapter(&self) -> Arc<dyn twentytoo_core::DataAdapter<Self::Entity>> {
                return self.adapter.clone();
            }
        }

        let resource = WidgetResource {
            adapter: Arc::new(InMemoryAdapter::new()),
        };
        let actor = Actor {
            id: "admin".to_string(),
            email: "admin@example.com".to_string(),
            roles: vec!["admin".to_string()],
            permissions: vec!["*.*".to_string()],
            team_id: None,
        };
        let view = ResourceView::for_actor(&resource, &actor);
        let nav = vec![NavItem {
            key: "widgets",
            label: "Widgets",
            icon: "cube",
        }];
        let pager = PagerView {
            mode: "numbered",
            current: 1,
            total_pages: Some(1),
            page_links: Vec::new(),
            prev_url: None,
            next_url: None,
        };
        let items = vec![serde_json::json!({
            "id": "w1", "name": "Gadget", "status": "active",
            "created_at": "2026-08-13T10:30:00Z",
        })];
        let values = serde_json::json!({ "name": "Gadget", "status": "active" });
        let errors: HashMap<String, String> = HashMap::new();

        let engine = TemplateEngine::new(None).expect("env builds");
        let renders: Vec<(&str, serde_json::Value)> = vec![
            (
                "layout/base.html.j2",
                serde_json::json!({ "nav": &nav, "active": "widgets", "actor": &actor }),
            ),
            (
                "dashboard/home.html.j2",
                serde_json::json!({ "cards": [], "nav": &nav, "active": "home", "actor": &actor }),
            ),
            (
                "resource/list.html.j2",
                serde_json::json!({
                    "resource": &view, "items": &items, "pager": &pager, "q": "",
                    "sort_param": "", "link_base": "", "can_create": true,
                    "nav": &nav, "active": "widgets", "actor": &actor,
                }),
            ),
            (
                "resource/detail.html.j2",
                serde_json::json!({
                    "resource": &view, "record": &items[0], "can_update": true,
                    "can_delete": true, "nav": &nav, "active": "widgets", "actor": &actor,
                }),
            ),
            (
                "resource/form.html.j2",
                serde_json::json!({
                    "resource": &view, "mode": "create", "form_action": "/resources/widgets",
                    "record_id": Option::<String>::None, "values": &values,
                    "errors": &errors, "form_error": Option::<String>::None,
                    "nav": &nav, "active": "widgets", "actor": &actor,
                }),
            ),
            (
                "partials/pagination.html.j2",
                serde_json::json!({ "pager": &pager }),
            ),
        ];
        for (name, ctx) in renders {
            let out = engine
                .render(name, &ctx)
                .unwrap_or_else(|e| panic!("render {name} failed: {e}"));
            assert!(!out.is_empty(), "{name} rendered empty");
        }
    }

    #[test]
    fn format_datetime_parses_rfc3339_and_passes_through_garbage() {
        let engine = TemplateEngine::new(None).unwrap();
        let ctx = serde_json::json!({});
        // format_datetime is a filter; exercise the underlying function
        // directly.
        let parsed = format_datetime(&Value::from("2026-08-13T10:30:00Z"), "%Y-%m-%d".to_string());
        assert_eq!(parsed, "2026-08-13");
        let raw = format_datetime(&Value::from("not a date"), "%Y".to_string());
        assert_eq!(raw, "not a date");
        let _ = engine;
        let _ = ctx;
    }

    #[test]
    fn currency_formats_money() {
        assert_eq!(currency(Value::from(1234.5)), "$1,234.50");
        assert_eq!(currency(Value::from(42)), "$42.00");
    }
}
