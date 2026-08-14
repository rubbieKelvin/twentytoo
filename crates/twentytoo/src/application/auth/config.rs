//! Workspace-facing auth configuration and the code-delivery seam.
//!
//! [`AuthConfig`] is what a workspace passes into the builder to turn auth
//! on; [`CodeSender`] is the pluggable hook for delivering a one-time login
//! code when email confirmation is enabled.

use async_trait::async_trait;

/// The bootstrap administrator the framework seeds when auth is enabled.
///
/// This is the framework's only self-creation path besides domain access:
/// an explicit email/password the workspace supplies in configuration.
#[derive(Clone, Debug)]
pub struct BootstrapAdmin {
    /// Login identity, stored lowercase.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Initial password (hashed before it is stored).
    pub password: String,
}

/// Auth behavior configuration, supplied by the workspace.
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// Whether the code step sits between the email and password steps.
    pub email_confirmation: bool,
    /// Domains whose unknown emails may self-create an account by
    /// attempting login. Empty = no domain self-creation.
    pub allowed_domains: Vec<String>,
    /// Optional administrator seeded at boot.
    pub bootstrap_admin: Option<BootstrapAdmin>,
    /// Lifetime of a login session.
    pub session_ttl: chrono::Duration,
    /// Lifetime of an in-progress login step token.
    pub step_token_ttl: chrono::Duration,
    /// Lifetime of an emailed login code.
    pub code_ttl: chrono::Duration,
}

impl Default for AuthConfig {
    fn default() -> Self {
        return Self {
            email_confirmation: false,
            allowed_domains: Vec::new(),
            bootstrap_admin: None,
            session_ttl: chrono::Duration::days(7),
            step_token_ttl: chrono::Duration::minutes(10),
            code_ttl: chrono::Duration::minutes(10),
        };
    }
}

/// Delivers a one-time login code to `email`.
///
/// Implementations send email or SMS; the framework's default
/// ([`ConsoleCodeSender`]) logs to stdout — no mail infrastructure ships
/// with this slice.
#[async_trait]
pub trait CodeSender: Send + Sync {
    /// Deliver the 6-digit login code to `email`.
    async fn send(&self, email: &str, code: &str);
}

/// The default [`CodeSender`]: prints the code to stdout.
pub struct ConsoleCodeSender;

#[async_trait]
#[allow(clippy::implicit_return)]
impl CodeSender for ConsoleCodeSender {
    async fn send(&self, email: &str, code: &str) {
        println!("[twentytoo] login code for {email}: {code}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_conservative() {
        let config = AuthConfig::default();
        assert!(!config.email_confirmation);
        assert!(config.allowed_domains.is_empty());
        assert!(config.bootstrap_admin.is_none());
        assert_eq!(config.session_ttl, chrono::Duration::days(7));
        assert_eq!(config.step_token_ttl, chrono::Duration::minutes(10));
        assert_eq!(config.code_ttl, chrono::Duration::minutes(10));
    }
}
