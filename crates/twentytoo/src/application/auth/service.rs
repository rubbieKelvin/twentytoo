//! The auth use cases: two-step login, code verification, and bootstrap.
//!
//! [`AuthService`] is the single handle handlers and middleware use. The
//! flow is email → (code, when configured) → password; each in-progress
//! step is anchored by a short-lived, single-use token stored in the
//! `login_tokens` table. Domain access lets an unknown email on an allowed
//! domain self-create an account by completing login, and `bootstrap`
//! seeds the administrator, `admin` role, and its permissions.

use chrono::Utc;
use serde_json::json;
use twentytoo_core::AuditAction;
use twentytoo_db::Db;
use twentytoo_db::DbError;
use twentytoo_db::entities::{LoginPurpose, NewAuditEntry, SessionInfo, User, UserStatus};
use uuid::Uuid;

use crate::application::auth::config::{AuthConfig, CodeSender};
use crate::application::auth::passwords::{hash_password, verify_password};
use crate::application::auth::tokens::{hash_code, hash_token, new_code, new_token};

/// The outcome of [`AuthService::start_login`], naming the next step.
#[derive(Debug)]
pub enum StartOutcome {
    /// The email step succeeded; the next step is the password screen.
    Password {
        /// Raw step token the client echoes back.
        token: String,
    },
    /// The email step succeeded and email confirmation is on; the next
    /// step is the code screen.
    Code {
        /// Raw step token the client echoes back.
        token: String,
    },
}

/// A login failure, translated into an HTTP response by the handlers.
#[derive(Debug)]
pub enum AuthError {
    /// No active account for the email and no domain allowance.
    UnknownEmail,
    /// Missing, expired, used, or wrong-purpose token (or attempts
    /// exhausted).
    BadToken,
    /// The emailed code did not match.
    BadCode,
    /// Five wrong codes; the token is consumed, restart from email.
    CodeLocked,
    /// The password did not match.
    BadPassword,
    /// A fresh-account password was under 8 characters.
    WeakPassword,
    /// A database failure.
    Db(DbError),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return match self {
            AuthError::UnknownEmail => write!(f, "no account found for that email"),
            AuthError::BadToken => write!(f, "invalid or expired login token"),
            AuthError::BadCode => write!(f, "incorrect code"),
            AuthError::CodeLocked => write!(f, "too many code attempts"),
            AuthError::BadPassword => write!(f, "incorrect password"),
            AuthError::WeakPassword => write!(f, "password is too weak"),
            AuthError::Db(e) => write!(f, "database error: {e}"),
        };
    }
}

impl std::error::Error for AuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        return match self {
            AuthError::Db(e) => Some(e),
            _ => None,
        };
    }
}

impl From<DbError> for AuthError {
    fn from(e: DbError) -> Self {
        return AuthError::Db(e);
    }
}

/// The password-login use cases over [`Db`].
pub struct AuthService {
    /// The database handle.
    pub(crate) db: Db,
    /// Workspace auth configuration.
    pub(crate) config: AuthConfig,
    /// Code delivery (stdout by default).
    pub(crate) code_sender: Box<dyn CodeSender>,
}

impl AuthService {
    /// Build the service from a database handle, configuration, and code
    /// sender.
    pub fn new(db: Db, config: AuthConfig, code_sender: Box<dyn CodeSender>) -> Self {
        return Self {
            db,
            config,
            code_sender,
        };
    }

    /// Step 1: resolve `email` and issue the next step token.
    ///
    /// Known active users get a token; invited/disabled users and unknown
    /// emails without domain allowance get [`AuthError::UnknownEmail`]; an
    /// unknown email on an allowed domain self-creates an account (no
    /// password yet) and audits the create. When email confirmation is on,
    /// a code is generated and delivered and [`StartOutcome::Code`] is
    /// returned; otherwise [`StartOutcome::Password`].
    pub async fn start_login(&self, email: &str) -> Result<StartOutcome, AuthError> {
        // Opportunistic cleanup of expired step tokens.
        let _ = self.db.delete_expired_login_tokens().await;

        let email = email.trim().to_lowercase();

        let user_id: Uuid = match self.db.get_user_by_email(&email).await? {
            Some(user) if user.status == UserStatus::Active => user.id,
            Some(_) => return Err(AuthError::UnknownEmail),
            None => {
                if !domain_allowed(&email, &self.config.allowed_domains) {
                    return Err(AuthError::UnknownEmail);
                }
                let user = self
                    .db
                    .create_user(&email, local_part(&email), None)
                    .await?;
                let id = user.id;
                self.record_create(&user).await?;
                id
            }
        };

        let token = new_token();
        let code: Option<String> = if self.config.email_confirmation {
            Some(new_code())
        } else {
            None
        };
        let code_hash: Option<String> = code.as_deref().map(hash_code);

        self.db
            .create_login_token(
                &hash_token(&token),
                &email,
                Some(&user_id),
                LoginPurpose::EmailOk,
                code_hash.as_deref(),
                Utc::now() + self.config.step_token_ttl,
            )
            .await?;

        if let Some(code) = code {
            self.code_sender.send(&email, &code).await;
            return Ok(StartOutcome::Code { token });
        }
        return Ok(StartOutcome::Password { token });
    }

    /// The code step (only when email confirmation is on).
    ///
    /// Verifies `code` against the stored hash; a wrong code bumps the
    /// attempt counter and locks (consumes) the token at 5. A correct code
    /// consumes the `email_ok` token and issues a fresh `code_ok` token,
    /// returning its raw value.
    pub async fn verify_code(&self, token: &str, code: &str) -> Result<String, AuthError> {
        let token_hash = hash_token(token);
        let Some(stored) = self.db.get_login_token(&token_hash).await? else {
            return Err(AuthError::BadToken);
        };

        if stored.purpose.as_str() != LoginPurpose::EmailOk.as_str() {
            return Err(AuthError::BadToken);
        }
        let Some(expected) = stored.code_hash.as_deref() else {
            return Err(AuthError::BadToken);
        };

        if constant_time_eq(hash_code(code).as_bytes(), expected.as_bytes()) {
            self.db.consume_login_token(&token_hash).await?;
            let fresh = new_token();
            self.db
                .create_login_token(
                    &hash_token(&fresh),
                    &stored.email,
                    stored.user_id.as_ref(),
                    LoginPurpose::CodeOk,
                    None,
                    Utc::now() + self.config.step_token_ttl,
                )
                .await?;
            return Ok(fresh);
        }

        let attempts = self.db.bump_login_token_attempts(&token_hash).await?;
        if attempts >= 5 {
            self.db.consume_login_token(&token_hash).await?;
            return Err(AuthError::CodeLocked);
        }
        return Err(AuthError::BadCode);
    }

    /// The password step: verify (or, for a fresh domain account, set) the
    /// password, then create the session and audit the login.
    pub async fn complete_login(
        &self,
        token: &str,
        password: &str,
    ) -> Result<(String, User), AuthError> {
        let token_hash = hash_token(token);
        let Some(stored) = self.db.get_login_token(&token_hash).await? else {
            return Err(AuthError::BadToken);
        };

        let expected_purpose: &str = if self.config.email_confirmation {
            LoginPurpose::CodeOk.as_str()
        } else {
            LoginPurpose::EmailOk.as_str()
        };
        if stored.purpose.as_str() != expected_purpose {
            return Err(AuthError::BadToken);
        }

        let Some(user_id) = stored.user_id else {
            return Err(AuthError::BadToken);
        };
        let Some(user) = self.db.get_user(&user_id).await? else {
            return Err(AuthError::BadToken);
        };

        if let Some(hash) = &user.password_hash {
            if !verify_password(password, hash) {
                let attempts = self.db.bump_login_token_attempts(&token_hash).await?;
                if attempts >= 5 {
                    self.db.consume_login_token(&token_hash).await?;
                    return Err(AuthError::BadToken);
                }
                return Err(AuthError::BadPassword);
            }
        } else {
            if password.len() < 8 {
                return Err(AuthError::WeakPassword);
            }
            let hash = hash_password(password).map_err(|e| {
                return AuthError::Db(DbError::Validation(format!("password hashing failed: {e}")));
            })?;
            self.db.set_user_password(&user.id, &hash).await?;
        }

        self.db.consume_login_token(&token_hash).await?;

        let session_token = new_token();
        self.db
            .create_session(
                &hash_token(&session_token),
                &user.id,
                None,
                Utc::now() + self.config.session_ttl,
                &SessionInfo::default(),
            )
            .await?;

        self.db
            .record_audit(&NewAuditEntry {
                actor_id: user.id.to_string(),
                actor_email: user.email.clone(),
                action: AuditAction::Login,
                resource: "users".to_string(),
                resource_id: user.id.to_string(),
                before: None,
                after: Some(json!({
                    "id": user.id.to_string(),
                    "email": user.email,
                })),
                ip: None,
            })
            .await?;

        return Ok((session_token, user));
    }

    /// Idempotently seed the administrator, `admin` role, and its
    /// permissions. A no-op when [`AuthConfig::bootstrap_admin`] is `None`.
    pub async fn bootstrap(&self) -> Result<(), DbError> {
        let Some(admin) = &self.config.bootstrap_admin else {
            return Ok(());
        };

        let role = match self.db.get_role_by_key("admin").await? {
            Some(role) => role,
            None => match self
                .db
                .create_role("admin", "Administrator", "Built-in administrator role")
                .await
            {
                Ok(role) => role,
                Err(DbError::Conflict(_)) => self
                    .db
                    .get_role_by_key("admin")
                    .await?
                    .expect("admin role exists after conflict"),
                Err(e) => return Err(e),
            },
        };

        let permissions: [(&str, &str); 3] = [
            ("users.view", "View users"),
            ("users.create", "Create users"),
            ("users.update", "Update users — name, status, password"),
        ];
        for (code, description) in permissions {
            let permission = match self.db.get_permission_by_code(code).await? {
                Some(permission) => permission,
                None => match self.db.create_permission(code, description).await {
                    Ok(permission) => permission,
                    Err(DbError::Conflict(_)) => self
                        .db
                        .get_permission_by_code(code)
                        .await?
                        .expect("permission exists after conflict"),
                    Err(e) => return Err(e),
                },
            };
            self.db.grant_permission(&role.id, &permission.id).await?;
        }

        let user = match self.db.get_user_by_email(&admin.email).await? {
            Some(user) => user,
            None => {
                let hash = hash_password(&admin.password).map_err(|e| {
                    return DbError::Validation(format!("password hashing failed: {e}"));
                })?;
                self.db
                    .create_user(&admin.email, &admin.name, Some(&hash))
                    .await?
            }
        };

        self.db.assign_role(&user.id, &role.id, None).await?;
        return Ok(());
    }

    /// Audit a user creation, acting as the new user itself (the only actor
    /// available at domain self-creation time).
    async fn record_create(&self, user: &User) -> Result<(), DbError> {
        self.db
            .record_audit(&NewAuditEntry {
                actor_id: user.id.to_string(),
                actor_email: user.email.clone(),
                action: AuditAction::Create,
                resource: "users".to_string(),
                resource_id: user.id.to_string(),
                before: None,
                after: Some(json!({
                    "id": user.id.to_string(),
                    "email": user.email,
                    "name": user.name,
                    "status": user.status.as_str(),
                })),
                ip: None,
            })
            .await?;
        return Ok(());
    }
}

/// Whether `email`'s domain (the part after the last `@`, lowercased)
/// exactly equals one of `domains` (each lowercased). An email without
/// `@`, or an empty `domains` list, never matches.
fn domain_allowed(email: &str, domains: &[String]) -> bool {
    if domains.is_empty() {
        return false;
    }
    let Some((_local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    let domain = domain.to_lowercase();
    return domains.iter().any(|allowed| {
        return allowed.trim().to_lowercase() == domain;
    });
}

/// The part of an email before the first `@` (or the whole string when
/// there is none), used as the display name for self-created accounts.
fn local_part(email: &str) -> &str {
    return match email.split_once('@') {
        Some((local, _)) => local,
        None => email,
    };
}

/// Byte-wise constant-time equality for two same-shape byte strings.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= *x ^ *y;
    }
    return diff == 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_allowed_exact_match() {
        let domains = vec!["example.com".to_string()];
        assert!(domain_allowed("alice@example.com", &domains));
    }

    #[test]
    fn domain_allowed_rejects_subdomain_impostor() {
        let domains = vec!["example.com".to_string()];
        assert!(!domain_allowed("alice@badexample.com", &domains));
        assert!(!domain_allowed("alice@evil-example.com", &domains));
    }

    #[test]
    fn domain_allowed_is_case_insensitive() {
        let domains = vec!["Example.COM".to_string()];
        assert!(domain_allowed("Alice@example.com", &domains));
    }

    #[test]
    fn domain_allowed_rejects_empty_domains() {
        assert!(!domain_allowed("alice@example.com", &[]));
    }

    #[test]
    fn domain_allowed_rejects_missing_at() {
        let domains = vec!["example.com".to_string()];
        assert!(!domain_allowed("no-at-sign", &domains));
    }

    #[test]
    fn domain_allowed_uses_last_at() {
        let domains = vec!["example.com".to_string()];
        assert!(domain_allowed("a@b@example.com", &domains));
    }
}
