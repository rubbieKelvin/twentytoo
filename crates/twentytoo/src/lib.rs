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

pub mod application;
pub mod container;
pub mod infrastructure;
pub mod presentation;
pub mod shared;

pub use crate::container::{ErasedResource, Twentytoo, TwentytooBuilder};
pub use crate::presentation::state::AppState;
pub use crate::shared::errors::{AppError, BuildError};

/// One-stop import for the common consumer surface.
pub mod prelude {
    pub use crate::container::{ErasedResource, Twentytoo, TwentytooBuilder};
    pub use crate::presentation::state::AppState;
    pub use crate::shared::errors::{AppError, BuildError};
    pub use twentytoo_core::prelude::*;
}
