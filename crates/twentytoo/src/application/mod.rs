//! The application layer: use cases over the [`twentytoo_core`] contract.
//!
//! Pure logic only. no axum, no templates. `dto` are the view-model
//! structs handlers render, `payload` turns form submissions into typed
//! entities with validation, and `query` builds filter trees from request
//! params. Nothing here depends on the rest of this crate.

pub mod dto;
pub mod payload;
pub mod query;
