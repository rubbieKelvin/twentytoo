//! GET / — the dashboard home, and the unmatched-route fallback.

use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;
use twentytoo_core::Actor;

use crate::presentation::state::AppState;
use crate::shared::errors::AppError;

/// GET / — the dashboard home.
pub async fn home_handler(
    State(app): State<AppState>,
    axum::Extension(actor): axum::Extension<Actor>,
) -> Result<Response, AppError> {
    let cards = app.registry.home_cards(&actor).await;
    let nav = app.nav_for(&actor);
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
