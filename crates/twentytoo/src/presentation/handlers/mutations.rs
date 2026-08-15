//! POST /{key}, /{key}/{id}, /{key}/{id}/delete — the write handlers.
//!
//! Create and update share the same shape: payload build → entity
//! validation → adapter write, with validation and conflict failures
//! re-rendering the form as 422 instead of erroring.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use twentytoo_core::{Actor, DataError, Resource, WriteContext};

use crate::application::payload;
use crate::application::payload::validate_entity;
use crate::shared::errors::AppError;

use super::helpers::{gate_resource, htmx_redirect, is_htmx, record_id, render_form_error};
use super::{FormData, ResourceState};

/// POST /{key} — create one record.
pub async fn create_handler<R: Resource>(
    State(st): State<ResourceState<R>>,
    axum::Extension(actor): axum::Extension<Actor>,
    headers: axum::http::HeaderMap,
    form: FormData,
) -> Result<Response, AppError> {
    let resource = &*st.resource;
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
                &headers,
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
            &headers,
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
            let location = format!("/{}/{}", resource.key(), id);
            if is_htmx(&headers) {
                return Ok(htmx_redirect(
                    &location,
                    "success",
                    &format!("Created {}", resource.label()),
                ));
            }
            return Ok(Redirect::to(&location).into_response());
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
                &headers,
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
                &headers,
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
    headers: axum::http::HeaderMap,
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
                &headers,
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
            &headers,
        );
    }

    let ctx = WriteContext {
        expected_version: None,
        idempotency_key: None,
        actor: Some(&actor),
    };
    match resource.adapter().update(&id, payload, &ctx).await {
        Ok(_) => {
            let location = format!("/{}/{id}", resource.key());
            if is_htmx(&headers) {
                return Ok(htmx_redirect(&location, "success", "Changes saved"));
            }
            return Ok(Redirect::to(&location).into_response());
        }
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
                &headers,
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
                &headers,
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
    headers: axum::http::HeaderMap,
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
    let location = format!("/{}", resource.key());
    if is_htmx(&headers) {
        return Ok(htmx_redirect(
            &location,
            "success",
            &format!("Deleted {}", resource.label()),
        ));
    }
    return Ok(Redirect::to(&location).into_response());
}
