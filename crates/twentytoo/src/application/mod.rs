//! The application layer: use cases over the [`twentytoo_core`] contract
//! and the [`twentytoo_db`] data layer.
//!
//! Pure logic only. no axum, no templates. `dto` are the view-model
//! structs handlers render, `payload` turns form submissions into typed
//! entities with validation, `query` builds filter trees from request
//! params, and `auth` is the password login flow over `twentytoo_db` —
//! still no HTTP concerns. Nothing here depends on the rest of this crate.

pub mod auth;
pub mod dto;
pub mod payload;
pub mod query;
