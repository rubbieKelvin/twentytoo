//! The user row and its account status.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// A user's account status.
///
/// Sign-in gating (reject `Invited` and `Disabled`) is the auth flow's
/// decision; these are the stored states it decides against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserStatus {
    /// May sign in.
    Active,
    /// Created by invite; no password set yet.
    Invited,
    /// Locked out.
    Disabled,
}

impl UserStatus {
    /// The stored column value.
    pub fn as_str(&self) -> &'static str {
        return match self {
            UserStatus::Active => "active",
            UserStatus::Invited => "invited",
            UserStatus::Disabled => "disabled",
        };
    }
}

impl TryFrom<String> for UserStatus {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        return match s.as_str() {
            "active" => Ok(UserStatus::Active),
            "invited" => Ok(UserStatus::Invited),
            "disabled" => Ok(UserStatus::Disabled),
            _ => Err(format!("unknown user status: {s}")),
        };
    }
}

/// One row of `users`.
#[derive(Clone, Debug, FromRow)]
pub struct User {
    /// Stable id.
    pub id: Uuid,
    /// Login identity, stored lowercase.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Password hash, `None` until the user sets one.
    pub password_hash: Option<String>,
    /// Account status.
    #[sqlx(try_from = "String")]
    pub status: UserStatus,
    /// Row creation time.
    pub created_at: DateTime<Utc>,
    /// Last update time.
    pub updated_at: DateTime<Utc>,
}
