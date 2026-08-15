//! GET /{key}/{id} — one record, computed columns materialized.

use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use twentytoo_core::{Actor, Resource};

use crate::application::dto::{ResourceView, materialize_computed};
use crate::shared::errors::AppError;

use super::ResourceState;
use super::helpers::gate_resource;

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
    let nav = st.app.nav_for(&actor);
    let ctx = context! {
        resource => &view,
        record => &value,
        can_update,
        can_delete,
        nav => &nav,
        active => resource.key(),
        actor => &actor,
        auth => st.app.auth.is_some(),
    };
    let html = st.app.templates.render("resource/detail.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}
