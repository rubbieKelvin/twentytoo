//! Shared handler internals: flag gating, validation, error re-renders,
//! filter parsing, and the pager. Private to the `handlers` module.

use std::collections::HashMap;

use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use serde_json::Value;
use twentytoo_core::{
    Actor, Field, FieldKind, FilterNode, FilterOp, FilterValue, Page, PaginationModes, Resource,
};

use crate::error::AppError;
use crate::payload;
use crate::view::{PageLink, PagerView, ResourceView};

use super::ResourceState;

/// Flag gate: a resource behind a disabled flag is 404 (`01` §4.2).
pub(super) async fn gate_resource<R: Resource>(
    st: &ResourceState<R>,
    resource: &R,
) -> Result<(), AppError> {
    if let Some(flag) = resource.flag()
        && !st.app.flags.enabled(flag)
    {
        return Err(AppError::NotFound);
    }
    return Ok(());
}

/// Entity-level validation: JSON → typed entity → validators → back.
pub(super) fn validate_entity<R: Resource>(
    fields: &[Field<R::Entity>],
    payload: &Value,
) -> Option<String> {
    let entity: R::Entity = match serde_json::from_value(payload.clone()) {
        Ok(e) => e,
        Err(e) => {
            return Some(format!("payload does not match the entity: {e}"));
        }
    };
    return payload::run_validators(fields, &entity);
}

/// Re-render the form with errors (422), keeping the submitted values.
///
/// The eight arguments are the full render input — mode, record id,
/// submitted values, field errors, entity error — plus the three context
/// handles; grouping them into a struct would just relocate the noise.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_form_error<R: Resource>(
    st: &ResourceState<R>,
    resource: &R,
    actor: &Actor,
    mode: &str,
    record_id: Option<&str>,
    values: &Value,
    errors: payload::FieldErrors,
    form_error: Option<String>,
) -> Result<Response, AppError> {
    let view = ResourceView::for_actor(resource, actor);
    let nav = st.app.registry.nav();
    let ctx = context! {
        resource => &view,
        mode,
        form_action => form_action(resource.key(), record_id),
        record_id => &record_id,
        values,
        errors => &errors,
        form_error => &form_error,
        nav => &nav,
        active => resource.key(),
        actor,
    };
    let html = st.app.templates.render("resource/form.html.j2", &ctx)?;
    return Ok((axum::http::StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response());
}

/// The form's POST target.
pub(super) fn form_action(key: &str, record_id: Option<&str>) -> String {
    return match record_id {
        Some(id) => format!("/{key}/{id}"),
        None => format!("/{key}"),
    };
}

/// The created record's id, for the post-create redirect. Entities always
/// carry `"id"` (the in-memory adapter guarantees it on create).
pub(super) fn record_id<E: serde::Serialize>(entity: &E) -> String {
    return serde_json::to_value(entity)
        .ok()
        .and_then(|v| return v.get("id").cloned())
        .and_then(|v| return v.as_str().map(|s| return s.to_string()))
        .unwrap_or_default();
}

/// Build the filter tree from declared specs + request params.
///
/// A spec is offered only when the source's `filter_ops` contains its
/// operator; unparseable param values are ignored (a bad filter is just no
/// filter). Range filters use `{field}_min` / `{field}_max` params on
/// numeric and date kinds.
pub(super) fn build_filter<E>(
    specs: &[twentytoo_core::FilterSpec],
    fields: &[Field<E>],
    filter_ops: &[FilterOp],
    params: &HashMap<String, String>,
) -> Option<FilterNode> {
    let mut nodes: Vec<FilterNode> = Vec::new();

    for spec in specs {
        if !filter_ops.contains(&spec.op) {
            continue;
        }
        let Some(field) = fields.iter().find(|f| return f.name == spec.field) else {
            continue;
        };
        if let Some(raw) = params.get(spec.field)
            && let Some(value) = coerce(&field.kind, raw)
        {
            let op = match spec.op {
                FilterOp::In | FilterOp::NotIn => FilterValue::In(vec![value_to_json(&value)]),
                _ => value,
            };
            nodes.push(FilterNode::Field {
                field: spec.field.to_string(),
                op: spec.op,
                value: op,
            });
        }
    }

    // Ranges: {field}_min / {field}_max on numeric/date kinds.
    for field in fields {
        if !matches!(
            field.kind,
            FieldKind::Number | FieldKind::Currency | FieldKind::Date | FieldKind::DateTime
        ) {
            continue;
        }
        let min = params.get(&format!("{}_min", field.name));
        let max = params.get(&format!("{}_max", field.name));
        if min.is_none() && max.is_none() {
            continue;
        }
        let range = FilterValue::Range {
            gt: None,
            gte: min
                .and_then(|s| return coerce(&field.kind, s))
                .map(|v| return value_to_json(&v)),
            lt: None,
            lte: max
                .and_then(|s| return coerce(&field.kind, s))
                .map(|v| return value_to_json(&v)),
        };
        nodes.push(FilterNode::Field {
            field: field.name.to_string(),
            op: FilterOp::Gte,
            value: range,
        });
    }

    return match nodes.len() {
        0 => None,
        1 => nodes.pop(),
        _ => Some(FilterNode::And(nodes)),
    };
}

/// Coerce a query param to a typed filter value by field kind.
pub(super) fn coerce(kind: &FieldKind, raw: &str) -> Option<FilterValue> {
    match kind {
        FieldKind::Number | FieldKind::Currency => {
            if let Ok(n) = raw.parse::<i64>() {
                return Some(FilterValue::Int(n));
            }
            return raw
                .parse::<f64>()
                .ok()
                .map(|n| return FilterValue::Float(n));
        }
        FieldKind::Boolean => match raw {
            "true" | "1" => return Some(FilterValue::Bool(true)),
            "false" | "0" => return Some(FilterValue::Bool(false)),
            _ => return None,
        },
        FieldKind::Date | FieldKind::DateTime => {
            return chrono::DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|dt| return FilterValue::DateTime(dt.with_timezone(&chrono::Utc)));
        }
        _ => {
            if raw.is_empty() {
                return None;
            }
            return Some(FilterValue::Str(raw.to_string()));
        }
    }
}

/// A coerced filter value as JSON (for `In`/`NotIn` operand lists).
pub(super) fn value_to_json(value: &FilterValue) -> serde_json::Value {
    return match value {
        FilterValue::Null => serde_json::Value::Null,
        FilterValue::Bool(b) => serde_json::Value::Bool(*b),
        FilterValue::Int(n) => serde_json::Value::from(*n),
        FilterValue::Float(n) => serde_json::Value::from(*n),
        FilterValue::Str(s) => serde_json::Value::String(s.clone()),
        FilterValue::DateTime(dt) => serde_json::Value::String(dt.to_rfc3339()),
        FilterValue::In(_) => serde_json::Value::Null,
        FilterValue::Range { .. } => serde_json::Value::Null,
    };
}

/// The pager for one page of results (`03` §4.3): numbered pages when the
/// source counts cheaply, prev/next otherwise.
pub(super) fn build_pager<E>(
    result: &Page<E>,
    params: &super::ListParams,
    extra: &HashMap<String, String>,
    mode: PaginationModes,
    link_base: &str,
    base_path: &str,
) -> PagerView {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(25).clamp(1, 100);

    match result.total {
        Some(total) if mode != PaginationModes::Cursor && total > 0 => {
            let total_pages = total.div_ceil(per_page as u64) as usize;
            let mut pages: Vec<usize> = Vec::new();
            let start = page.saturating_sub(4);
            let end = (start + 9).min(total_pages).max(start + 1);
            for n in start..end {
                pages.push(n + 1);
            }
            return PagerView {
                mode: "numbered",
                current: page,
                total_pages: Some(total_pages),
                page_links: pages
                    .iter()
                    .map(|n| {
                        return PageLink {
                            page: *n,
                            url: format!("{base_path}?{link_base}page={n}"),
                        };
                    })
                    .collect(),
                prev_url: if page > 1 {
                    Some(format!("{base_path}?{link_base}page={}", page - 1))
                } else {
                    None
                },
                next_url: if page < total_pages {
                    Some(format!("{base_path}?{link_base}page={}", page + 1))
                } else {
                    None
                },
            };
        }
        _ => {
            let link = |key: &str, cursor: &str| -> Option<String> {
                if cursor.is_empty() {
                    return None;
                }
                let mut rest = extra.clone();
                rest.remove("after");
                rest.remove("before");
                rest.insert(key.to_string(), cursor.to_string());
                let qs = serde_urlencoded::to_string(&rest).unwrap_or_default();
                return Some(format!("{base_path}?{qs}"));
            };
            let prev = result.prev.as_ref().map(|c| return c.0.clone());
            let next = result.next.as_ref().map(|c| return c.0.clone());
            return PagerView {
                mode: "prevnext",
                current: page,
                total_pages: None,
                page_links: Vec::new(),
                prev_url: prev.and_then(|c| return link("before", &c)),
                next_url: next.and_then(|c| return link("after", &c)),
            };
        }
    }
}
