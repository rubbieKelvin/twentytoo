//! The typed access layer: the `impl Db` methods that operate over the
//! [`crate::entities`] row shapes — one module per domain.

pub mod access;
pub mod audit;
pub mod groups;
pub mod sessions;
pub mod users;
