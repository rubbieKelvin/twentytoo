//! Cross-cutting concerns shared by all layers.
//!
//! `errors` (the HTTP-facing `AppError` and boot-time `BuildError`) and
//! `utils` (small string/number helpers) have no home inside any single
//! hexagonal layer, so they live here and every layer may use them.

pub mod errors;
pub mod utils;
