//! Generic CRUD handlers: one implementation for every resource (`00` §7.3).
//!
//! Handlers are generic over `Resource` + its adapter; each resource gets a
//! monomorphized sub-router carrying [`ResourceState`]. The capability
//! matrix (`Capabilities`, read once at boot) drives pagination mode,
//! search, sort, and filters — the same handler drives an offset source
//! with numbered pages and a cursor-only source with prev/next (`00` §7.3).
//!
//! Layout: one module per concern — [`list`] (GET /{key}), [`detail`]
//! (GET /{key}/{id}), [`forms`] (create/edit form GETs), [`mutations`]
//! (create/update/delete POSTs), [`home`] (dashboard + fallback), and
//! [`helpers`] (shared internals). The extractors live in
//! [`crate::presentation::extractors`], the middleware in
//! [`crate::presentation::middleware`], and the per-resource route table
//! here.

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use twentytoo_core::Resource;

use crate::presentation::state::AppState;

pub mod auth;
mod detail;
mod forms;
mod helpers;
mod home;
mod list;

mod mutations;
pub mod users;

pub use crate::presentation::extractors::{FormData, ListParams};
pub use detail::detail_handler;

pub use forms::{create_form_handler, edit_form_handler};
pub use home::{home_handler, not_found};
pub use list::list_handler;

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

/// Build the per-resource route table.
pub fn resource_routes<R: Resource>() -> Router<ResourceState<R>> {
    return Router::new()
        .route("/", get(list_handler::<R>).post(create_handler::<R>))
        .route("/new", get(create_form_handler::<R>))
        .route("/{id}", get(detail_handler::<R>).post(update_handler::<R>))
        .route("/{id}/edit", get(edit_form_handler::<R>))
        .route("/{id}/delete", post(delete_handler::<R>));
}
