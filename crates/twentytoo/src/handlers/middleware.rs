//! The request pipeline: middleware that runs for every route.

use axum::extract::State;
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

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
