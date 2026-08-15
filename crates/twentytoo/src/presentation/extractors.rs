//! Request extractors: the form body and list-query params.

use std::collections::HashMap;

use axum::extract::RawForm;
use serde::{Deserialize, Serialize};

use crate::shared::errors::AppError;

/// A form body as field → values.
///
/// Repeated keys collect into vectors (multi-selects, checkbox groups);
/// single values arrive as one-element vectors. Extracted from the raw
/// body because `serde_urlencoded` cannot deserialize a scalar into
/// `Vec<String>` itself.
#[derive(Debug)]
pub struct FormData(pub HashMap<String, Vec<String>>);

impl std::ops::Deref for FormData {
    type Target = HashMap<String, Vec<String>>;

    fn deref(&self) -> &Self::Target {
        return &self.0;
    }
}

impl<S> axum::extract::FromRequest<S> for FormData
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let raw = RawForm::from_request(req, state).await?;
        let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(&raw.0)
            .map_err(|e| return AppError::BadRequest(format!("malformed form body: {e}")))?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (key, value) in pairs {
            map.entry(key).or_default().push(value);
        }
        return Ok(Self(map));
    }
}

/// List-view query params.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ListParams {
    /// 1-based page number (offset mode).
    pub page: Option<usize>,
    /// Rows per page (clamped to 1..=100).
    pub per_page: Option<usize>,
    /// Sort key; `-` prefix means descending.
    pub sort: Option<String>,
    /// Search term.
    pub q: Option<String>,
}

/// A one-shot toast carried by a redirect (`?flash=<kind>:<message>`).
///
/// Mutations 303 to their destination with the flash query param; the
/// base layout renders it as a Tabler toast on the landed page. `kind`
/// is one of `"success"`, `"danger"`, `"info"` — anything else parses
/// as an empty flash (no toast). The param is framework-generated, but
/// it is still request input: never trust it, never render it raw.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Flash {
    /// Toast accent: `"success"`, `"danger"`, or `"info"`.
    pub kind: String,
    /// The message; empty when the request carried no flash.
    pub message: String,
}

impl Flash {
    /// The redirect target for a mutation: `location` plus the encoded
    /// flash payload.
    pub fn redirect(location: &str, kind: &str, message: &str) -> String {
        let qs = serde_urlencoded::to_string([("flash", format!("{kind}:{message}"))])
            .unwrap_or_default();
        return format!("{location}?{qs}");
    }

    /// Parse `?flash=<kind>:<message>` out of a query string; a missing,
    /// malformed, or unknown-kind payload yields an empty flash.
    fn parse(query: Option<&str>) -> Self {
        let Some(q) = query else {
            return Self::default();
        };
        let Ok(pairs) = serde_urlencoded::from_str::<HashMap<String, String>>(q) else {
            return Self::default();
        };
        let Some(raw) = pairs.get("flash") else {
            return Self::default();
        };
        let Some((kind, message)) = raw.split_once(':') else {
            return Self::default();
        };
        if !matches!(kind, "success" | "danger" | "info") {
            return Self::default();
        }
        return Self {
            kind: kind.to_string(),
            message: message.to_string(),
        };
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Flash {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        return Ok(Self::parse(parts.uri.query()));
    }
}
