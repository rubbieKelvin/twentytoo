//! Twentytoo: the internal-tools dashboard framework.
//!
//! Define a resource — entity, fields, actions, policy, adapter — and get
//! generated CRUD views with role-gated access. This crate is the HTTP
//! layer: generic handlers over the [`twentytoo_core`] contract, a
//! MiniJinja template engine with framework functions, and a builder that
//! assembles the axum router with boot-time validation.
//!
//! ```
//! use twentytoo::prelude::*;
//!
//! let status: Field<serde_json::Value> = field!(
//!     "status",
//!     "Status",
//!     Badge { options: &[("open", "Open"), ("closed", "Closed")] },
//!     list: true,
//! );
//! let all = fields![
//!     status,
//!     field!("id", "Id", Text, required: true),
//! ];
//!
//! assert_eq!(all.len(), 2);
//! assert!(all[0].show_in_list);
//! assert!(all[1].required);
//! ```

pub use twentytoo_core::*;

pub mod app;
pub mod error;
pub mod flags;
pub mod handlers;
pub mod payload;
pub mod registry;
pub mod state;
pub mod templates;
pub mod util;
pub mod view;

pub use crate::app::{ErasedResource, Twentytoo, TwentytooBuilder};
pub use crate::error::{AppError, BuildError};
pub use crate::state::AppState;

/// One-stop import for the common consumer surface.
pub mod prelude {
    pub use crate::app::{ErasedResource, Twentytoo, TwentytooBuilder};
    pub use crate::error::{AppError, BuildError};
    pub use crate::state::AppState;
    pub use twentytoo_core::prelude::*;
}
