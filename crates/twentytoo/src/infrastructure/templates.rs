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
    "dashboard/home.html.j2",
    "resource/list.html.j2",
    "resource/detail.html.j2",
    "resource/form.html.j2",
    "partials/pagination.html.j2",
    "auth/email.html.j2",
    "auth/code.html.j2",
    "auth/password.html.j2",
    "users/list.html.j2",
    "users/form.html.j2",
];

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
        env.add_function("format_field", format_field);
        env.add_function("format_filter", format_filter);
        env.add_function("form_control", form_control);
        env.add_filter("format_datetime", format_datetime);
        env.add_filter("currency", currency);

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
        "currency" => match as_f64(&value) {
            Some(n) => format_money(n),
            None => value.to_string(),
        },
        "datetime" => format_datetime(&value, "%Y-%m-%d %H:%M".to_string()),
        "boolean" => {
            if value.is_true() {
                "Yes".to_string()
            } else {
                "No".to_string()
            }
        }
        "select" | "badge" => label_for(kind, value.as_str()),
        "multiselect" => {
            let mut labels: Vec<String> = value
                .try_iter()
                .map(|iter| return iter.map(|v| return label_for(kind, v.as_str())).collect())
                .unwrap_or_default();
            if labels.is_empty() {
                labels.push(value.to_string());
            }
            labels.join(", ")
        }
        "relation" => {
            let id = escape_html(&value.to_string());
            let resource_key = escape_html(kind.relation.as_deref().unwrap_or(""));
            if resource_key.is_empty() {
                id
            } else {
                format!("<a href=\"/{resource_key}/{id}\">{id}</a>")
            }
        }
        // text, textarea, richtext, number, date, email, json, file,
        // image, computed — plain escaped text (computed values were
        // materialized into the row by the handler).
        _ => escape_html(&value.to_string()),
    };
    return Value::from_safe_string(text);
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

/// Template function: `format_filter(filter)` — the sidebar control for one
/// filter. Safe string; values and labels escaped internally.
fn format_filter(filter: ViaDeserialize<FilterView>) -> Value {
    let f = &*filter;
    let name = escape_html(&f.name);
    let label = escape_html(&f.label);
    let current = escape_html(f.current.as_deref().unwrap_or(""));
    let html = match f.op.as_str() {
        "eq" | "in" | "notin"
            if matches!(f.kind.tag.as_str(), "select" | "badge" | "multiselect") =>
        {
            let mut out =
                format!("<label class=\"filter\"><span>{label}</span><select name=\"{name}\">");
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
            out.push_str("</select></label>");
            out
        }
        "gt" | "gte" | "lt" | "lte" if matches!(f.kind.tag.as_str(), "number" | "currency") => {
            format!(
                "<label class=\"filter\"><span>{label}</span><input type=\"number\" name=\"{name}\" value=\"{current}\" placeholder=\"{label}\"></label>"
            )
        }
        "gt" | "gte" | "lt" | "lte" if matches!(f.kind.tag.as_str(), "date") => {
            format!(
                "<label class=\"filter\"><span>{label}</span><input type=\"date\" name=\"{name}\" value=\"{current}\"></label>"
            )
        }
        _ => {
            format!(
                "<label class=\"filter\"><span>{label}</span><input type=\"text\" name=\"{name}\" value=\"{current}\" placeholder=\"{label}\"></label>"
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

/// Template function: `form_control(field, values)` — one form widget.
/// Safe string; current values escaped internally.
fn form_control(field: ViaDeserialize<FieldView>, values: Value) -> Value {
    let f = &*field;
    let name = escape_html(&f.name);
    let id = format!("f-{name}");
    let current = values.get_attr(&f.name).unwrap_or_default();
    let html = match f.kind.tag.as_str() {
        "boolean" => {
            let checked = if current.is_true() { " checked" } else { "" };
            format!("<input type=\"checkbox\" id=\"{id}\" name=\"{name}\"{checked}>")
        }
        "textarea" | "richtext" => {
            let v = escape_html(&value_string(&current));
            format!("<textarea id=\"{id}\" name=\"{name}\" rows=\"5\">{v}</textarea>")
        }
        "select" | "badge" => {
            let mut out = format!("<select id=\"{id}\" name=\"{name}\">");
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
            let mut out = format!("<select id=\"{id}\" name=\"{name}\" multiple size=\"4\">");
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
                "<input type=\"number\" step=\"any\" id=\"{id}\" name=\"{name}\" value=\"{v}\">"
            )
        }
        "date" => {
            let v = escape_html(&value_string(&current));
            format!("<input type=\"date\" id=\"{id}\" name=\"{name}\" value=\"{v}\">")
        }
        "datetime" => {
            let v = escape_html(&value_string(&current));
            format!(
                "<input type=\"text\" id=\"{id}\" name=\"{name}\" value=\"{v}\" placeholder=\"2026-08-13T10:30:00Z\">"
            )
        }
        "email" => {
            let v = escape_html(&value_string(&current));
            format!("<input type=\"email\" id=\"{id}\" name=\"{name}\" value=\"{v}\">")
        }
        // text, json, relation, computed — plain text input; file/image
        // kinds are excluded from forms by the view model.
        _ => {
            let v = escape_html(&value_string(&current));
            format!("<input type=\"text\" id=\"{id}\" name=\"{name}\" value=\"{v}\">")
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
                    "resource": &view, "mode": "create", "form_action": "/widgets",
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
