//! Shared handler internals: flag gating, validation, error re-renders,
//! filter parsing, and the pager. Private to the `handlers` module.

use std::collections::HashMap;

use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use serde_json::Value;
use twentytoo_core::{Actor, Page, PaginationModes, Resource};

use crate::application::dto::{PageLink, PagerView, ResourceView};
use crate::application::payload;
use crate::shared::errors::AppError;

use super::{FormData, ResourceState};

/// The first submitted value of a form field (single-value fields arrive
/// as one-element vectors).
pub(super) fn single_value<'a>(form: &'a FormData, name: &str) -> Option<&'a str> {
    return form
        .get(name)
        .and_then(|v| return v.first())
        .map(|s| return s.as_str());
}

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
    let nav = st.app.nav_for(actor);
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
