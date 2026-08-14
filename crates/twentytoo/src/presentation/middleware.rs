//! The request pipeline: middleware that runs for every route.

use axum::extract::State;
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use twentytoo_core::Actor;

use crate::application::auth::{AuthService, hash_token};
use crate::presentation::state::AppState;
use crate::shared::utils::read_cookie;

/// The session cookie the login flow sets and every auth-aware request
/// carries back.
const SESSION_COOKIE: &str = "twentytoo_session";

/// Resolves the request's identity.
///
/// Without auth configured: injects the configured default actor — the
/// byte-identical path from before auth existed.
///
/// With auth configured: reads the `twentytoo_session` cookie, resolves it
/// to a session, and injects the expanded actor. Requests without a valid
/// session are denied — `302 /login` for GET/HEAD, `401` otherwise —
/// except the public auth routes themselves (`/login`, `/login/*`,
/// `/logout`), which run without an actor extension.
pub async fn actor_layer(
    State(app): State<AppState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(auth) = &app.auth else {
        request.extensions_mut().insert(app.default_actor.clone());
        return next.run(request).await;
    };

    if let Some(actor) = session_actor(auth, request.headers()).await {
        request.extensions_mut().insert(actor);
        return next.run(request).await;
    }

    let path = request.uri().path();
    if is_public_path(path) {
        return next.run(request).await;
    }
    if request.method() == Method::GET || request.method() == Method::HEAD {
        return Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, "/login")
            .body(axum::body::Body::empty())
            .expect("static redirect builds");
    }
    return (
        StatusCode::UNAUTHORIZED,
        Html("<!doctype html><html><body><h1>401</h1><p>Sign in required</p></body></html>"),
    )
        .into_response();
}

/// The actor for a valid, unexpired session cookie — `None` on a missing
/// cookie, unknown session, unknown user, or database failure (deny).
async fn session_actor(auth: &AuthService, headers: &axum::http::HeaderMap) -> Option<Actor> {
    let token = read_cookie(headers, SESSION_COOKIE)?;
    let session = auth
        .db
        .get_session(&hash_token(&token))
        .await
        .ok()
        .flatten()?;
    return auth
        .db
        .load_actor(&session.user_id, None)
        .await
        .ok()
        .flatten();
}

/// Whether `path` is reachable without a session: the auth routes plus the
/// embedded static assets — the login pages load the framework stylesheet
/// before any actor exists.
fn is_public_path(path: &str) -> bool {
    return path == "/login"
        || path.starts_with("/login/")
        || path == "/logout"
        || path.starts_with("/static/");
}

#[cfg(test)]
mod tests {
    use super::is_public_path;

    #[test]
    fn public_paths_cover_auth_routes_and_static_assets() {
        assert!(is_public_path("/login"));
        assert!(is_public_path("/login/code"));
        assert!(is_public_path("/logout"));
        assert!(is_public_path("/static/css/app.css"));
        assert!(is_public_path("/static/js/htmx.min.js"));
    }

    #[test]
    fn everything_else_is_protected() {
        assert!(!is_public_path("/"));
        assert!(!is_public_path("/users"));
        assert!(!is_public_path("/loginish"));
        assert!(!is_public_path("/stores/42"));
    }
}
