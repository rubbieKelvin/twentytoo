//! Concrete implementations of services the framework uses.
//!
//! `templates` is the MiniJinja environment (framework templates, function
//! library, boot-time validation), `static_files` the embedded-asset
//! service behind `/static`, and `flags` the runtime flag service. All sit
//! below `presentation/`, which consumes them through `AppState`.

pub mod flags;
pub mod static_files;
pub mod templates;
