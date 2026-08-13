//! Generic CRUD handlers: one implementation for every resource (`01` §4.2).
//!
//! Handlers are generic over `Resource` + its adapter; each resource gets a
//! monomorphized sub-router carrying `ResourceState<R>`. The capability
//! matrix (`Capabilities`, read once at boot) drives pagination mode,
//! search, sort, and filters — the same handler drives an offset source
//! with numbered pages and a cursor-only source with prev/next (`03` §14.1).

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, RawForm, State};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use minijinja::context;
use serde::Deserialize;
use serde_json::Value;
use twentytoo_core::{
    Actor, DataError, Field, FieldKind, FilterNode, FilterOp, FilterValue, NullsOrder, Page,
    Pagination, PaginationModes, Query as DataQuery, Resource, SearchMode, SearchSpec, SortDir,
    SortField, WriteContext,
};

use crate::error::AppError;
use crate::payload;
use crate::state::AppState;
use crate::view::{PageLink, PagerView, ResourceView, materialize_computed};

/// Per-resource handler state: the app plus one concrete resource.
pub struct ResourceState<R: Resource> {
    /// Shared app state (templates, flags, registry).
    pub app: Arc<AppState>,
    /// The resource this router serves.
    pub resource: Arc<R>,
}

impl<R: Resource> Clone for ResourceState<R> {
    fn clone(&self) -> Self {
        return Self {
            app: self.app.clone(),
            resource: self.resource.clone(),
        };
    }
}

/// A form body as field → values.
///
/// Repeated keys collect into vectors (multi-selects, checkbox groups);
/// single values arrive as one-element vectors. Extracted from the raw
/// body because `serde_urlencoded` cannot deserialize a scalar into
/// `Vec<String>` itself.
#[derive(Debug)]
pub struct FormData(pub HashMap<String, Vec<String>>);

impl std::ops::Deref for FormData {
    type Target = HashMap<String, Vec<String>>;

    fn deref(&self) -> &Self::Target {
        return &self.0;
    }
}

impl<S> axum::extract::FromRequest<S> for FormData
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let raw = RawForm::from_request(req, state).await?;
        let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(&raw.0)
            .map_err(|e| return AppError::BadRequest(format!("malformed form body: {e}")))?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (key, value) in pairs {
            map.entry(key).or_default().push(value);
        }
        return Ok(Self(map));
    }
}

/// List-view query params.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ListParams {
    /// 1-based page number (offset mode).
    pub page: Option<usize>,
    /// Rows per page (clamped to 1..=100).
    pub per_page: Option<usize>,
    /// Sort key; `-` prefix means descending.
    pub sort: Option<String>,
    /// Search term.
    pub q: Option<String>,
}

/// Build the per-resource route table.
pub fn resource_routes<R: Resource>() -> Router<ResourceState<R>> {
    return Router::new()
        .route("/", get(list_handler::<R>).post(create_handler::<R>))
        .route("/new", get(create_form_handler::<R>))
        .route("/{id}", get(detail_handler::<R>).post(update_handler::<R>))
        .route("/{id}/edit", get(edit_form_handler::<R>))
        .route("/{id}/delete", post(delete_handler::<R>));
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// GET /{key} — one page of rows.
pub async fn list_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
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
    let nav = st.app.registry.nav();
    let ctx = context! {
        resource => &view,
        items => &result.items,
        pager => &pager,
        q => params.q.as_deref().unwrap_or(""),
        sort_param => params.sort.clone().unwrap_or_default(),
        link_base => &link_base,
        can_create => resource.policy().can_create(&actor),
        nav => &nav,
        active => resource.key(),
        actor => &actor,
    };
    let html = st.app.templates.render("resource/list.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

/// GET /{key}/{id} — one record.
pub async fn detail_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    let record = resource
        .adapter()
        .get(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !resource.policy().can_view(&actor, &record) {
        return Err(AppError::Forbidden);
    }

    let mut value =
        serde_json::to_value(&record).map_err(|e| return AppError::Internal(Box::new(e)))?;
    materialize_computed(&resource.fields(), &mut value);

    let view = ResourceView::for_actor(resource, &actor);
    let can_update = resource.policy().can_update(&actor, &record);
    let can_delete = resource.policy().can_delete(&actor, &record);
    let nav = st.app.registry.nav();
    let ctx = context! {
        resource => &view,
        record => &value,
        can_update,
        can_delete,
        nav => &nav,
        active => resource.key(),
        actor => &actor,
    };
    let html = st.app.templates.render("resource/detail.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

// ---------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------

/// GET /{key}/new — the create form.
pub async fn create_form_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    if !resource.policy().can_create(&actor) {
        return Err(AppError::Forbidden);
    }
    let view = ResourceView::for_actor(resource, &actor);
    let nav = st.app.registry.nav();
    let ctx = context! {
        resource => &view,
        mode => "create",
        form_action => format!("/{}", resource.key()),
        record_id => Option::<String>::None,
        values => &Value::Object(Default::default()),
        errors => &payload::FieldErrors::new(),
        form_error => Option::<String>::None,
        nav => &nav,
        active => resource.key(),
        actor => &actor,
    };
    let html = st.app.templates.render("resource/form.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

/// GET /{key}/{id}/edit — the edit form, pre-filled.
pub async fn edit_form_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    let record = resource
        .adapter()
        .get(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !resource.policy().can_update(&actor, &record) {
        return Err(AppError::Forbidden);
    }
    let values =
        serde_json::to_value(&record).map_err(|e| return AppError::Internal(Box::new(e)))?;
    let view = ResourceView::for_actor(resource, &actor);
    let nav = st.app.registry.nav();
    let ctx = context! {
        resource => &view,
        mode => "edit",
        form_action => format!("/{}/{}", resource.key(), id),
        record_id => &id,
        values => &values,
        errors => &payload::FieldErrors::new(),
        form_error => Option::<String>::None,
        nav => &nav,
        active => resource.key(),
        actor => &actor,
    };
    let html = st.app.templates.render("resource/form.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// POST /{key} — create one record.
pub async fn create_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    form: FormData,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    if !resource.policy().can_create(&actor) {
        return Err(AppError::Forbidden);
    }

    let fields = resource.fields();
    let payload = match payload::build_payload(&fields, &form) {
        Ok(p) => p,
        Err(errors) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "create",
                None,
                &payload::form_values(&form),
                errors,
                None,
            );
        }
    };
    if let Some(msg) = validate_entity::<R>(&fields, &payload) {
        return render_form_error(
            &st,
            resource,
            &actor,
            "create",
            None,
            &payload::form_values(&form),
            payload::FieldErrors::new(),
            Some(msg),
        );
    }

    let ctx = WriteContext {
        expected_version: None,
        idempotency_key: None,
        actor: Some(&actor),
    };
    match resource.adapter().create(payload, &ctx).await {
        Ok(created) => {
            let id = record_id(&created);
            return Ok(Redirect::to(&format!("/{}/{}", resource.key(), id)).into_response());
        }
        Err(DataError::Validation(msg)) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "create",
                None,
                &payload::form_values(&form),
                payload::FieldErrors::new(),
                Some(msg),
            );
        }
        Err(DataError::Conflict) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "create",
                None,
                &payload::form_values(&form),
                payload::FieldErrors::new(),
                Some("A record with this id already exists.".to_string()),
            );
        }
        Err(e) => return Err(e.into()),
    }
}

/// POST /{key}/{id} — update one record.
pub async fn update_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
    form: FormData,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    let record = resource
        .adapter()
        .get(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !resource.policy().can_update(&actor, &record) {
        return Err(AppError::Forbidden);
    }

    let fields = resource.fields();
    let payload = match payload::build_payload(&fields, &form) {
        Ok(p) => p,
        Err(errors) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "edit",
                Some(&id),
                &payload::form_values(&form),
                errors,
                None,
            );
        }
    };
    if let Some(msg) = validate_entity::<R>(&fields, &payload) {
        return render_form_error(
            &st,
            resource,
            &actor,
            "edit",
            Some(&id),
            &payload::form_values(&form),
            payload::FieldErrors::new(),
            Some(msg),
        );
    }

    let ctx = WriteContext {
        expected_version: None,
        idempotency_key: None,
        actor: Some(&actor),
    };
    match resource.adapter().update(&id, payload, &ctx).await {
        Ok(_) => return Ok(Redirect::to(&format!("/{}/{id}", resource.key())).into_response()),
        Err(DataError::Validation(msg)) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "edit",
                Some(&id),
                &payload::form_values(&form),
                payload::FieldErrors::new(),
                Some(msg),
            );
        }
        Err(DataError::Conflict) => {
            return render_form_error(
                &st,
                resource,
                &actor,
                "edit",
                Some(&id),
                &payload::form_values(&form),
                payload::FieldErrors::new(),
                Some("This record changed elsewhere — reload and retry.".to_string()),
            );
        }
        Err(e) => return Err(e.into()),
    }
}

/// POST /{key}/{id}/delete — remove one record.
pub async fn delete_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
    gate_resource(&st, resource).await?;
    let record = resource
        .adapter()
        .get(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !resource.policy().can_delete(&actor, &record) {
        return Err(AppError::Forbidden);
    }
    let ctx = WriteContext {
        expected_version: None,
        idempotency_key: None,
        actor: Some(&actor),
    };
    resource.adapter().delete(&id, &ctx).await?;
    return Ok(Redirect::to(&format!("/{}", resource.key())).into_response());
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

/// GET / — the dashboard home.
pub async fn home_handler(
    State(app): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
) -> Result<Response, AppError> {
    let cards = app.registry.home_cards(&actor).await;
    let nav = app.registry.nav();
    let ctx = context! {
        cards => &cards,
        nav => &nav,
        active => "home",
        actor => &actor,
    };
    let html = app.templates.render("dashboard/home.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

/// The fallback: everything unmatched is 404.
pub async fn not_found() -> AppError {
    return AppError::NotFound;
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Injects the configured default actor into every request.
///
/// Sessions and real actor extraction arrive with auth (`01` Step 5); until
/// then the framework assumes the configured identity.
pub async fn actor_layer(
    State(app): State<AppState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(app.default_actor.clone());
    return next.run(request).await;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Flag gate: a resource behind a disabled flag is 404 (`01` §4.2).
async fn gate_resource<R: Resource>(st: &ResourceState<R>, resource: &R) -> Result<(), AppError> {
    if let Some(flag) = resource.flag()
        && !st.app.flags.enabled(flag)
    {
        return Err(AppError::NotFound);
    }
    return Ok(());
}

/// Entity-level validation: JSON → typed entity → validators → back.
fn validate_entity<R: Resource>(fields: &[Field<R::Entity>], payload: &Value) -> Option<String> {
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
fn render_form_error<R: Resource>(
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
fn form_action(key: &str, record_id: Option<&str>) -> String {
    return match record_id {
        Some(id) => format!("/{key}/{id}"),
        None => format!("/{key}"),
    };
}

/// The created record's id, for the post-create redirect. Entities always
/// carry `"id"` (the in-memory adapter guarantees it on create).
fn record_id<E: serde::Serialize>(entity: &E) -> String {
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
fn build_filter<E>(
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
fn coerce(kind: &FieldKind, raw: &str) -> Option<FilterValue> {
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
fn value_to_json(value: &FilterValue) -> serde_json::Value {
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
fn build_pager<E>(
    result: &Page<E>,
    params: &ListParams,
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
