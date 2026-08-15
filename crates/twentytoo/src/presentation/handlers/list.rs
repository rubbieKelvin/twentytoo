//! GET /{key} — one page of rows, with pagination, sort, search, filters.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use twentytoo_core::{
    Actor, NullsOrder, Pagination, PaginationModes, Query as DataQuery, Resource, SearchMode,
    SearchSpec, SortDir, SortField,
};

use crate::application::dto::ResourceView;
use crate::application::query::build_filter;
use crate::shared::errors::AppError;

use super::helpers::{build_pager, gate_resource};
use super::{ListParams, ResourceState};

/// GET /{key} — one page of rows.
pub async fn list_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    headers: axum::http::HeaderMap,
    Query(params): Query<ListParams>,
    Query(extra): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    if !resource.policy().can_view_any(&actor) {
        return Err(AppError::Forbidden);
    }

    let caps = resource.adapter().capabilities();
    let fields = resource.fields();
    let per_page = params.per_page.unwrap_or(25).clamp(1, 100);
    let page = params.page.unwrap_or(1).max(1);

    // Pagination: offset when the source offers it, cursors otherwise.
    let pagination = match caps.pagination {
        PaginationModes::Offset | PaginationModes::Both => Pagination::Offset { page, per_page },
        PaginationModes::Cursor => Pagination::Cursor {
            after: extra.get("after").cloned(),
            before: extra.get("before").cloned(),
            per_page,
        },
    };

    // Sort: request wins when the field exists, is sortable, and the source
    // sorts at all; otherwise the resource's default.
    let mut sort: Vec<SortField> = Vec::new();
    if caps.sort
        && let Some(raw) = &params.sort
    {
        let (dir, name) = match raw.strip_prefix('-') {
            Some(rest) => (SortDir::Desc, rest),
            None => (SortDir::Asc, raw.as_str()),
        };
        if let Some(f) = fields.iter().find(|f| return f.name == name)
            && f.sortable
        {
            sort.push(SortField {
                field: name.to_string(),
                dir,
                nulls: NullsOrder::Default,
            });
        }
    }
    if sort.is_empty() {
        sort = resource.default_sort();
    }

    // Search: term over the resource's search fields, when the source has a
    // search mode and the resource declares fields.
    let search = match (caps.search, params.q.as_deref()) {
        (SearchMode::None, _) | (_, None) => None,
        (_, Some(term)) if term.trim().is_empty() => None,
        (_, Some(term)) => {
            let search_fields = resource.search_fields();
            if search_fields.is_empty() {
                None
            } else {
                Some(SearchSpec {
                    term: term.trim().to_string(),
                    fields: search_fields.iter().map(|s| return s.to_string()).collect(),
                })
            }
        }
    };

    let filter = build_filter(&resource.filters(), &fields, &caps.filter_ops, &extra);

    // Projection: the visible columns; "id" is always included — it powers
    // detail links and row identity.
    let mut projection: Vec<String> = resource
        .list_columns()
        .iter()
        .map(|s| return s.to_string())
        .collect();
    if !projection.iter().any(|c| return c == "id") {
        projection.push("id".to_string());
    }

    let query = DataQuery {
        pagination,
        sort,
        filter,
        search,
        projection: Some(projection),
    };
    let result = resource.adapter().list(&query).await?;

    // Preserved params for links: everything except page/sort.
    let mut rest = extra.clone();
    rest.remove("page");
    rest.remove("sort");
    let link_base = if rest.is_empty() {
        String::new()
    } else {
        format!(
            "{}&",
            serde_urlencoded::to_string(&rest).unwrap_or_default()
        )
    };

    let view = ResourceView::for_actor(resource, &actor).with_filter_values(&extra);
    let base_path = format!("/{}", resource.key());
    let pager = build_pager(
        &result,
        &params,
        &extra,
        caps.pagination,
        &link_base,
        &base_path,
    );
    let nav = st.app.nav_for(&actor);
    let has_filters = !extra.is_empty();
    let ctx = context! {
        resource => &view,
        items => &result.items,
        pager => &pager,
        q => params.q.as_deref().unwrap_or(""),
        has_filters,
        sort_param => params.sort.clone().unwrap_or_default(),
        link_base => &link_base,
        can_create => resource.policy().can_create(&actor),
        nav => &nav,
        active => resource.key(),
        actor => &actor,
        auth => st.app.auth.is_some(),
    };
    // htmx list controls (search/filter/sort/pagination) swap the bare
    // #list fragment; boosted and plain GETs render the full page — the
    // layout's hx-select picks out #main for boosted swaps (`01-ui-kit`
    // §8.2/§8.3).
    let fragment = headers.get("HX-Request").is_some() && headers.get("HX-Boosted").is_none();
    let name = if fragment {
        "partials/list.html.j2"
    } else {
        "resource/list.html.j2"
    };
    let html = st.app.templates.render(name, &ctx)?;
    return Ok(Html(html).into_response());
}
