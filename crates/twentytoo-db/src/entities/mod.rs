//! The row shapes for the framework-owned tables — the data model, split
//! from the query/access modules so each entity is its own concept.

pub mod audit;
pub mod group;
pub mod permission;
pub mod role;
pub mod session;
pub mod user;

pub use crate::entities::audit::NewAuditEntry;
pub use crate::entities::group::Group;
pub use crate::entities::permission::Permission;
pub use crate::entities::role::Role;
pub use crate::entities::session::{Session, SessionInfo};
pub use crate::entities::user::{User, UserStatus};
