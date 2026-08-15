//! The Users area: the framework-owned user management screens behind the
//! auth service. Every route gates on the actor's permissions and runs
//! against `users`/`inapp_events` through the auth service's database handle;
//! the routes only exist when auth is configured.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use minijinja::context;
use serde_json::{Value, json};
use twentytoo_core::{Actor, AuditAction};
use twentytoo_db::DbError;
use twentytoo_db::entities::{NewAuditEntry, User, UserStatus};
use uuid::Uuid;

use crate::application::auth::{AuthService, hash_password};
use crate::application::payload::FieldErrors;
use crate::presentation::extractors::FormData;
use crate::presentation::state::AppState;
use crate::shared::errors::AppError;

use super::helpers::{htmx_redirect, is_htmx, single_value};

/// GET /users — one row per account, with a create link for actors holding
/// `users.create`.
pub async fn list_handler(
    State(st): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
) -> Result<Response, AppError> {
    if !actor.can("users.view") {
        return Err(AppError::Forbidden);
    }
    let auth = auth_of(&st);
    let rows = auth
        .db
        .list_users()
        .await
        .map_err(|e| return AppError::Data(e.into()))?;
    let users: Vec<Value> = rows
        .iter()
        .map(|u| {
            return json!({
                "id": u.id.to_string(),
                "email": u.email,
                "name": u.name,
                "status": u.status.as_str(),
                "created_at": u.created_at.to_rfc3339(),
            });
        })
        .collect();
    let can_create = actor.can("users.create");
    let nav = st.nav_for(&actor);
    let ctx = context! {
        users => &users,
        can_create,
        nav => &nav,
        active => "users",
        actor => &actor,
        auth => st.auth.is_some(),
    };
    let html = st.templates.render("users/list.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

/// GET /users/new — the create form.
pub async fn create_form(
    State(st): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AppError> {
    if !actor.can("users.create") {
        return Err(AppError::Forbidden);
    }
    return render_user_form(
        &st,
        &actor,
        "create",
        None,
        &json!({}),
        FieldErrors::new(),
        None,
        StatusCode::OK,
        &headers,
    );
}

/// POST /users/new — create an account with an initial password, write the
/// audit entry, and land on the user's edit screen.
pub async fn create_handler(
    State(st): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
    headers: axum::http::HeaderMap,
    form: FormData,
) -> Result<Response, AppError> {
    if !actor.can("users.create") {
        return Err(AppError::Forbidden);
    }
    let email = single_value(&form, "email").unwrap_or_default();
    let name = single_value(&form, "name").unwrap_or_default();
    let password = single_value(&form, "password").unwrap_or_default();

    let mut errors = FieldErrors::new();
    if email.is_empty() || !email.contains('@') {
        errors.insert(
            "email".to_string(),
            "A valid email address is required.".to_string(),
        );
    }
    if name.is_empty() {
        errors.insert("name".to_string(), "Name is required.".to_string());
    }
    if password.len() < 8 {
        errors.insert(
            "password".to_string(),
            "Password must be at least 8 characters.".to_string(),
        );
    }
    if !errors.is_empty() {
        let values = json!({"email": email, "name": name});
        return render_user_form(
            &st,
            &actor,
            "create",
            None,
            &values,
            errors,
            None,
            StatusCode::UNPROCESSABLE_ENTITY,
            &headers,
        );
    }

    let auth = auth_of(&st);
    let hash = hash_password(password).map_err(|e| return AppError::Internal(Box::new(e)))?;
    let user = match auth.db.create_user(email, name, Some(&hash)).await {
        Ok(user) => user,
        Err(DbError::Conflict(_)) => {
            let mut errors = FieldErrors::new();
            errors.insert(
                "email".to_string(),
                "A user with that email already exists.".to_string(),
            );
            let values = json!({"email": email, "name": name});
            return render_user_form(
                &st,
                &actor,
                "create",
                None,
                &values,
                errors,
                None,
                StatusCode::CONFLICT,
                &headers,
            );
        }
        Err(e) => return Err(AppError::Data(e.into())),
    };

    let entry = NewAuditEntry {
        actor_id: actor.id.clone(),
        actor_email: actor.email.clone(),
        action: AuditAction::Create,
        resource: "users".to_string(),
        resource_id: user.id.to_string(),
        before: None,
        after: Some(json!({
            "id": user.id.to_string(),
            "email": user.email,
            "name": user.name,
            "status": user.status.as_str(),
        })),
        ip: None,
    };
    auth.db
        .record_audit(&entry)
        .await
        .map_err(|e| return AppError::Data(e.into()))?;
    let location = format!("/users/{}", user.id);
    if is_htmx(&headers) {
        return Ok(htmx_redirect(&location, "success", "User created"));
    }
    return Ok(Redirect::to(&location).into_response());
}

/// GET /users/{id} — the edit form (name, status, password).
pub async fn edit_form(
    State(st): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AppError> {
    if !actor.can("users.update") {
        return Err(AppError::Forbidden);
    }
    let user_id = parse_id(&id)?;
    let auth = auth_of(&st);
    let user = auth
        .db
        .get_user(&user_id)
        .await
        .map_err(|e| return AppError::Data(e.into()))?
        .ok_or(AppError::NotFound)?;
    let values = json!({"name": user.name, "status": user.status.as_str()});
    return render_user_form(
        &st,
        &actor,
        "edit",
        Some(&user),
        &values,
        FieldErrors::new(),
        None,
        StatusCode::OK,
        &headers,
    );
}

/// POST /users/{id} — apply name/status/password edits. Disabling one's
/// own account is rejected.
pub async fn update_handler(
    State(st): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    form: FormData,
) -> Result<Response, AppError> {
    if !actor.can("users.update") {
        return Err(AppError::Forbidden);
    }
    let user_id = parse_id(&id)?;
    let name = single_value(&form, "name").unwrap_or_default();
    let status_raw = single_value(&form, "status").unwrap_or_default();
    let password = single_value(&form, "password").unwrap_or_default();

    let auth = auth_of(&st);
    let user = auth
        .db
        .get_user(&user_id)
        .await
        .map_err(|e| return AppError::Data(e.into()))?
        .ok_or(AppError::NotFound)?;

    let mut errors = FieldErrors::new();
    if name.is_empty() {
        errors.insert("name".to_string(), "Name is required.".to_string());
    }
    let status: Option<UserStatus> = match status_raw {
        "active" => Some(UserStatus::Active),
        "disabled" => Some(UserStatus::Disabled),
        _ => {
            errors.insert(
                "status".to_string(),
                "Status must be \"active\" or \"disabled\".".to_string(),
            );
            None
        }
    };
    if !password.is_empty() && password.len() < 8 {
        errors.insert(
            "password".to_string(),
            "Password must be at least 8 characters.".to_string(),
        );
    }
    if !errors.is_empty() {
        let values = json!({"name": name, "status": status_raw});
        return render_user_form(
            &st,
            &actor,
            "edit",
            Some(&user),
            &values,
            errors,
            None,
            StatusCode::UNPROCESSABLE_ENTITY,
            &headers,
        );
    }
    let status = status.expect("validated: status is active or disabled");

    // An admin must never lock themselves out by accident.
    if status == UserStatus::Disabled && user.id.to_string() == actor.id {
        let mut errors = FieldErrors::new();
        errors.insert(
            "status".to_string(),
            "You cannot disable your own account.".to_string(),
        );
        let values = json!({"name": name, "status": status_raw});
        return render_user_form(
            &st,
            &actor,
            "edit",
            Some(&user),
            &values,
            errors,
            None,
            StatusCode::UNPROCESSABLE_ENTITY,
            &headers,
        );
    }

    if !password.is_empty() {
        let hash = hash_password(password).map_err(|e| return AppError::Internal(Box::new(e)))?;
        auth.db
            .set_user_password(&user_id, &hash)
            .await
            .map_err(|e| return AppError::Data(e.into()))?;
    }
    auth.db
        .update_user_name(&user_id, name)
        .await
        .map_err(|e| return AppError::Data(e.into()))?;
    if user.status != status {
        auth.db
            .set_user_status(&user_id, status)
            .await
            .map_err(|e| return AppError::Data(e.into()))?;
    }
    if is_htmx(&headers) {
        return Ok(htmx_redirect("/users", "success", "User updated"));
    }
    return Ok(Redirect::to("/users").into_response());
}

/// The shared auth service; the routes only exist when it does.
fn auth_of(st: &AppState) -> &AuthService {
    return st
        .auth
        .as_deref()
        .expect("users routes mount only when auth is enabled");
}

/// A path id as a user id; malformed ids are simply not found.
fn parse_id(id: &str) -> Result<Uuid, AppError> {
    return Uuid::parse_str(id).map_err(|_| return AppError::NotFound);
}

/// Render the shared create/edit form. `user` supplies the edit-mode facts
/// (record id, email display); `values` the current field values. The
/// arguments are the full render input — grouping them into a struct would
/// just relocate the noise (same shape as `render_form_error`). htmx posts
/// get the bare form fragment swapped into `#form-region`; plain GETs and
/// posts get the full page (`01-ui-kit` §8.7).
#[allow(clippy::too_many_arguments)]
fn render_user_form(
    st: &AppState,
    actor: &Actor,
    mode: &str,
    user: Option<&User>,
    values: &Value,
    errors: FieldErrors,
    form_error: Option<String>,
    status: StatusCode,
    headers: &axum::http::HeaderMap,
) -> Result<Response, AppError> {
    let nav = st.nav_for(actor);
    let record_id = user.map(|u| return u.id.to_string());
    let email = user.map(|u| return u.email.clone());
    let ctx = context! {
        mode,
        values,
        errors,
        form_error,
        record_id,
        email,
        nav => &nav,
        active => "users",
        actor,
        auth => st.auth.is_some(),
    };
    let name = if is_htmx(headers) {
        "partials/users-form.html.j2"
    } else {
        "users/form.html.j2"
    };
    let html = st.templates.render(name, &ctx)?;
    return Ok((status, Html(html)).into_response());
}
