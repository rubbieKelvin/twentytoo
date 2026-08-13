//! The session row and the tracking facts captured at creation.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

/// One row of `sessions`.
#[derive(Clone, Debug, FromRow)]
pub struct Session {
    /// Hash of the session token, not the token itself.
    pub token_hash: String,
    /// The user the session belongs to.
    pub user_id: Uuid,
    /// The group this session acts within, if any.
    pub group_id: Option<Uuid>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Sessions past this point are invalid.
    pub expires_at: DateTime<Utc>,
    /// Last activity time (refreshed by [`crate::Db::touch_session`]).
    pub last_seen_at: Option<DateTime<Utc>>,
    /// The client's user-agent at creation.
    pub user_agent: Option<String>,
    /// The client's address at creation, as a string.
    pub ip: Option<String>,
    /// The page that led to sign-in, if sent.
    pub referrer: Option<String>,
    /// The client's `Accept-Language` header, if sent.
    pub accept_language: Option<String>,
    /// Device label, e.g. `"iPhone"` or `"Desktop"`.
    pub device: Option<String>,
    /// Operating system, e.g. `"macOS"`.
    pub os: Option<String>,
    /// Browser, e.g. `"Chrome"`.
    pub browser: Option<String>,
    /// Arbitrary extra tracking data (extra headers, geolocation,
    /// correlation ids, …). An empty object when nothing was recorded.
    pub metadata: Value,
}

/// Optional tracking facts captured at session creation. Every field is
/// optional; build one with the struct-update syntax or [`Default`].
#[derive(Clone, Debug)]
pub struct SessionInfo {
    /// The client's user-agent.
    pub user_agent: Option<String>,
    /// The client's address, as a string.
    pub ip: Option<String>,
    /// The page that led to sign-in.
    pub referrer: Option<String>,
    /// The client's `Accept-Language` header.
    pub accept_language: Option<String>,
    /// Device label.
    pub device: Option<String>,
    /// Operating system.
    pub os: Option<String>,
    /// Browser.
    pub browser: Option<String>,
    /// Arbitrary extra tracking data.
    pub metadata: Value,
}

impl Default for SessionInfo {
    fn default() -> Self {
        return Self {
            user_agent: None,
            ip: None,
            referrer: None,
            accept_language: None,
            device: None,
            os: None,
            browser: None,
            metadata: Value::Object(serde_json::Map::new()),
        };
    }
}
