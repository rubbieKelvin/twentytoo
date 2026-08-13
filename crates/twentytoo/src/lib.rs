//! Twentytoo: the internal-tools framework facade.
//!
//! Re-exports the core contract. Handlers, templates, and the module system
//! arrive in later slices.
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

/// One-stop import for the common consumer surface.
pub mod prelude {
    pub use twentytoo_core::prelude::*;
}
