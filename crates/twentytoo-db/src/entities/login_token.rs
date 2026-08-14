//! The login-token row and its step purpose.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// Which step of the login flow a token proves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPurpose {
    /// The email step is confirmed; the flow may proceed to the code
    /// (email confirmation) or password step.
    EmailOk,
    /// The code step is confirmed; the flow may proceed to the password
    /// step.
    CodeOk,
}

impl LoginPurpose {
    /// The stored column value.
    pub fn as_str(&self) -> &'static str {
        return match self {
            LoginPurpose::EmailOk => "email_ok",
            LoginPurpose::CodeOk => "code_ok",
        };
    }
}

impl TryFrom<String> for LoginPurpose {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        return match s.as_str() {
            "email_ok" => Ok(LoginPurpose::EmailOk),
            "code_ok" => Ok(LoginPurpose::CodeOk),
            _ => Err(format!("unknown login purpose: {s}")),
        };
    }
}

/// One row of `login_tokens`.
#[derive(Clone, Debug, FromRow)]
pub struct LoginToken {
    /// Hash of the step token, not the token itself.
    pub token_hash: String,
    /// The login email the token was issued for.
    pub email: String,
    /// The resolved user, `None` until the email step finds one.
    pub user_id: Option<Uuid>,
    /// Which step the token proves.
    #[sqlx(try_from = "String")]
    pub purpose: LoginPurpose,
    /// Hash of the emailed code, when email confirmation is on.
    pub code_hash: Option<String>,
    /// Number of wrong-code/wrong-password attempts.
    pub attempts: i32,
    /// When the token was consumed; `None` until then.
    pub used_at: Option<DateTime<Utc>>,
    /// Tokens past this point are invalid.
    pub expires_at: DateTime<Utc>,
    /// Row creation time.
    pub created_at: DateTime<Utc>,
}
