//! Auth handlers: the two-step login flow (email → [code] → password) and
//! logout. Mounted on the outer router only when auth is configured; every
//! screen renders through the standalone auth layout (no topbar) and never
//! extracts an actor — the middleware lets these paths through without one.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use serde_json::json;
use twentytoo_core::AuditAction;
use twentytoo_db::entities::NewAuditEntry;

use crate::application::auth::{AuthError, AuthService, StartOutcome, hash_token};
use crate::presentation::extractors::FormData;
use crate::presentation::registry::NavItem;
use crate::presentation::state::AppState;
use crate::shared::errors::AppError;
use crate::shared::utils::{is_secure_request, read_cookie, set_cookie};

use super::helpers::single_value;

/// The cookie carrying the raw step token between login steps.
const STEP_COOKIE: &str = "twentytoo_login_step";
/// The cookie carrying the session token after a successful login.
const SESSION_COOKIE: &str = "twentytoo_session";

/// GET /login — the email step; a request already carrying a valid session
/// is sent home instead.
pub async fn login_screen(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    if let Some(token) = read_cookie(&headers, SESSION_COOKIE)
        && auth
            .db
            .get_session(&hash_token(&token))
            .await
            .map_err(db_err)?
            .is_some()
    {
        return redirect("/", &[]);
    }
    return render_screen(&st, "auth/email.html.j2", None, StatusCode::OK, &[]);
}

/// POST /login/email — step 1: resolve the email (creating the account
/// when the domain allows), issue the step token, and route to the code or
/// password step.
pub async fn login_email_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    form: FormData,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    let email = single_value(&form, "email").unwrap_or_default();
    let secure = is_secure_request(&headers, &uri);
    let max_age = auth.config.step_token_ttl.num_seconds() as u64;
    match auth.start_login(email).await {
        Ok(StartOutcome::Password { token }) => {
            let cookie = set_cookie(STEP_COOKIE, &token, max_age, secure);
            return redirect("/login/password", &[cookie]);
        }
        Ok(StartOutcome::Code { token }) => {
            let cookie = set_cookie(STEP_COOKIE, &token, max_age, secure);
            return redirect("/login/code", &[cookie]);
        }
        Err(AuthError::UnknownEmail) => {
            // One message for unknown + inactive accounts: no enumeration.
            return render_screen(
                &st,
                "auth/email.html.j2",
                Some("No account found with that email address."),
                StatusCode::OK,
                &[],
            );
        }
        Err(AuthError::Db(e)) => return Err(AppError::Data(e.into())),
        Err(_) => return Err(AppError::BadRequest("unexpected login error".to_string())),
    }
}

/// POST /login/code — the code step (email confirmation on): verify the
/// emailed code and exchange the token for a password-step token.
pub async fn login_code_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    form: FormData,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    let Some(token) = read_cookie(&headers, STEP_COOKIE) else {
        return redirect("/login", &[]);
    };
    let code = single_value(&form, "code").unwrap_or_default();
    let secure = is_secure_request(&headers, &uri);
    let max_age = auth.config.step_token_ttl.num_seconds() as u64;
    match auth.verify_code(&token, code).await {
        Ok(new_token) => {
            let cookie = set_cookie(STEP_COOKIE, &new_token, max_age, secure);
            return redirect("/login/password", &[cookie]);
        }
        Err(AuthError::BadCode) => {
            return render_screen(
                &st,
                "auth/code.html.j2",
                Some("Incorrect code."),
                StatusCode::UNAUTHORIZED,
                &[],
            );
        }
        Err(AuthError::CodeLocked) => {
            return render_screen(
                &st,
                "auth/code.html.j2",
                Some("Too many attempts. Start over."),
                StatusCode::UNAUTHORIZED,
                &[clear_cookie(STEP_COOKIE)],
            );
        }
        Err(AuthError::BadToken) => return redirect("/login", &[clear_cookie(STEP_COOKIE)]),
        Err(AuthError::Db(e)) => return Err(AppError::Data(e.into())),
        Err(_) => return Err(AppError::BadRequest("unexpected login error".to_string())),
    }
}

/// GET /login/code — the code screen (email confirmation on). Missing or
/// invalid step token → back to `/login`.
pub async fn code_screen(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    let Some(token) = read_cookie(&headers, STEP_COOKIE) else {
        return redirect("/login", &[]);
    };
    if auth
        .db
        .get_login_token(&hash_token(&token))
        .await
        .map_err(db_err)?
        .is_none()
    {
        return redirect("/login", &[]);
    }
    return render_screen(&st, "auth/code.html.j2", None, StatusCode::OK, &[]);
}

/// GET /login/password — the password screen. Missing or invalid step
/// token → back to `/login`. The mode is `"set"` for accounts without a
/// password yet (fresh domain accounts) and `"verify"` otherwise.
pub async fn password_screen(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    let Some(token) = read_cookie(&headers, STEP_COOKIE) else {
        return redirect("/login", &[]);
    };
    let Some(step) = auth
        .db
        .get_login_token(&hash_token(&token))
        .await
        .map_err(db_err)?
    else {
        return redirect("/login", &[]);
    };
    let Some(user_id) = step.user_id else {
        return redirect("/login", &[]);
    };
    let Some(user) = auth.db.get_user(&user_id).await.map_err(db_err)? else {
        return redirect("/login", &[]);
    };
    let mode = if user.password_hash.is_none() {
        "set"
    } else {
        "verify"
    };
    let ctx = context! {
        mode,
        error => Option::<String>::None,
    };
    let html = st.templates.render("auth/password.html.j2", &ctx)?;
    return Ok(Html(html).into_response());
}

/// POST /login/password — the final step: verify (or set) the password,
/// open a session, and clear the step token.
pub async fn login_password_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    form: FormData,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    let Some(token) = read_cookie(&headers, STEP_COOKIE) else {
        return redirect("/login", &[]);
    };
    let password = single_value(&form, "password").unwrap_or_default();
    let secure = is_secure_request(&headers, &uri);
    let max_age = auth.config.session_ttl.num_seconds() as u64;
    match auth.complete_login(&token, password).await {
        Ok((session_token, _user)) => {
            let session = set_cookie(SESSION_COOKIE, &session_token, max_age, secure);
            return redirect("/", &[clear_cookie(STEP_COOKIE), session]);
        }
        Err(AuthError::BadPassword) => {
            return render_password(
                &st,
                "verify",
                Some("Incorrect password."),
                StatusCode::UNAUTHORIZED,
                &[],
            );
        }
        Err(AuthError::WeakPassword) => {
            return render_password(
                &st,
                "set",
                Some("Password must be at least 8 characters."),
                StatusCode::UNPROCESSABLE_ENTITY,
                &[],
            );
        }
        Err(AuthError::BadToken | AuthError::CodeLocked) => {
            return redirect("/login", &[clear_cookie(STEP_COOKIE)]);
        }
        Err(AuthError::Db(e)) => return Err(AppError::Data(e.into())),
        Err(_) => return Err(AppError::BadRequest("unexpected login error".to_string())),
    }
}

/// POST /logout — delete the session (auditing the sign-out), clear both
/// cookies, and return to the login screen.
pub async fn logout_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let auth = auth_of(&st);
    if let Some(token) = read_cookie(&headers, SESSION_COOKIE) {
        let hash = hash_token(&token);
        let session = auth.db.get_session(&hash).await.map_err(db_err)?;
        let _ = auth.db.delete_session(&hash).await;
        if let Some(session) = session
            && let Some(user) = auth.db.get_user(&session.user_id).await.ok().flatten()
        {
            let entry = NewAuditEntry {
                actor_id: user.id.to_string(),
                actor_email: user.email.clone(),
                action: AuditAction::Logout,
                resource: "users".to_string(),
                resource_id: user.id.to_string(),
                before: None,
                after: Some(json!({"id": user.id.to_string(), "email": user.email})),
                ip: None,
            };
            // A failed audit write must not block signing out.
            let _ = auth.db.record_audit(&entry).await;
        }
    }
    return redirect(
        "/login",
        &[clear_cookie(SESSION_COOKIE), clear_cookie(STEP_COOKIE)],
    );
}

/// The shared auth service; the routes only exist when it does.
fn auth_of(st: &AppState) -> &AuthService {
    return st
        .auth
        .as_deref()
        .expect("auth routes mount only when auth is enabled");
}

/// `DbError` → `AppError` (via the core `DataError` mapping).
fn db_err(e: twentytoo_db::DbError) -> AppError {
    return AppError::Data(e.into());
}

/// A cleared cookie (empty value, `Max-Age=0`).
fn clear_cookie(name: &str) -> String {
    return set_cookie(name, "", 0, false);
}

/// `302 Location: <target>`, plus the given `Set-Cookie` headers.
fn redirect(target: &str, cookies: &[String]) -> Result<Response, AppError> {
    let response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, target)
        .body(axum::body::Body::empty())
        .map_err(|e| return AppError::Internal(Box::new(e)))?;
    return with_cookies(response, cookies);
}

/// Render an auth screen (email/code) with an optional error banner.
fn render_screen(
    st: &AppState,
    template: &str,
    error: Option<&str>,
    status: StatusCode,
    cookies: &[String],
) -> Result<Response, AppError> {
    let ctx = context! {
        error,
        nav => Vec::<NavItem>::new(),
    };
    let html = st.templates.render(template, &ctx)?;
    return with_cookies((status, Html(html)).into_response(), cookies);
}

/// Render the password screen for `mode` ("verify"/"set").
fn render_password(
    st: &AppState,
    mode: &str,
    error: Option<&str>,
    status: StatusCode,
    cookies: &[String],
) -> Result<Response, AppError> {
    let ctx = context! {
        mode,
        error,
        nav => Vec::<NavItem>::new(),
    };
    let html = st.templates.render("auth/password.html.j2", &ctx)?;
    return with_cookies((status, Html(html)).into_response(), cookies);
}

/// Append `Set-Cookie` headers to an existing response.
fn with_cookies(mut response: Response, cookies: &[String]) -> Result<Response, AppError> {
    for cookie in cookies {
        let value = axum::http::HeaderValue::from_str(cookie)
            .map_err(|e| return AppError::Internal(Box::new(e)))?;
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    return Ok(response);
}
