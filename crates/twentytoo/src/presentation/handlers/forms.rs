//! GET /{key}/new and /{key}/{id}/edit — the form pages, empty or pre-filled.

use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use serde_json::Value;
use twentytoo_core::{Actor, Resource};

use crate::application::dto::ResourceView;
use crate::application::payload;
use crate::shared::errors::AppError;

use super::ResourceState;
use super::helpers::gate_resource;

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
    let nav = st.app.nav_for(&actor);
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
        auth => st.app.auth.is_some(),
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
    let nav = st.app.nav_for(&actor);
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
        auth => st.app.auth.is_some(),
    };
    let html = st.app.templates.render("resource/form.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}
