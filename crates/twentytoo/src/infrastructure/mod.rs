//! Concrete implementations of services the framework uses.
//!
//! `templates` is the MiniJinja environment (framework templates, function
//! library, boot-time validation) and `flags` the runtime flag service.
//! Both sit below `presentation/`, which consumes them through `AppState`.

pub mod flags;
pub mod templates;
