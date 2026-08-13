//! Request extractors: the form body and list-query params.

use std::collections::HashMap;

use axum::extract::RawForm;
use serde::Deserialize;

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
