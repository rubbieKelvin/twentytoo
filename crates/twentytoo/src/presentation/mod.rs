//! The presentation layer: the HTTP-facing surface of the framework.
//!
//! `handlers` are the generic CRUD use cases, `middleware` the request
//! pipeline, `extractors` the form/query param parsing, `state` the router
//! state (`AppState`), and `registry` the nav/home DTOs assembled at boot.

pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod registry;
pub mod state;
