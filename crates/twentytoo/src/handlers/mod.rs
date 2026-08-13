//! Generic CRUD handlers: one implementation for every resource (`01` §4.2).
//!
//! Handlers are generic over `Resource` + its adapter; each resource gets a
//! monomorphized sub-router carrying [`ResourceState`]. The capability
//! matrix (`Capabilities`, read once at boot) drives pagination mode,
//! search, sort, and filters — the same handler drives an offset source
//! with numbered pages and a cursor-only source with prev/next (`03` §14.1).
//!
//! Layout: one module per concern — [`list`] (GET /{key}), [`detail`]
//! (GET /{key}/{id}), [`forms`] (create/edit form GETs), [`mutations`]
//! (create/update/delete POSTs), [`home`] (dashboard + fallback),
//! [`middleware`] (request pipeline), and [`helpers`] (shared internals).
//! The shared extractors and the per-resource route table live here.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::RawForm;
use axum::routing::{get, post};
use serde::Deserialize;
use twentytoo_core::Resource;

use crate::error::AppError;
use crate::state::AppState;

mod detail;
mod forms;
mod helpers;
mod home;
mod list;
mod middleware;
mod mutations;

pub use detail::detail_handler;
pub use forms::{create_form_handler, edit_form_handler};
pub use home::{home_handler, not_found};
pub use list::list_handler;
pub use middleware::actor_layer;
pub use mutations::{create_handler, delete_handler, update_handler};

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
