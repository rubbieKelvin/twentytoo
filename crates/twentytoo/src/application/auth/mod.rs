//! Password login: the pure use cases over [`twentytoo_db`]'s users,
//! sessions, roles, permissions, and login-token tables.
//!
//! No HTTP concerns live here — the handlers and middleware (wired in a
//! later slice) call [`AuthService`] and translate its [`AuthError`]s into
//! responses. The module is split by responsibility: `config` holds the
//! workspace-facing settings, `tokens` mints opaque step tokens and codes,
//! `passwords` wraps argon2 hashing, and `service` owns the use cases.

pub mod config;
pub mod passwords;
pub mod service;
pub mod tokens;

pub use crate::application::auth::config::{
    AuthConfig, BootstrapAdmin, CodeSender, ConsoleCodeSender,
};
pub use crate::application::auth::passwords::{hash_password, verify_password};
pub use crate::application::auth::service::{AuthError, AuthService, StartOutcome};
pub use crate::application::auth::tokens::{hash_code, hash_token, new_code, new_token};
