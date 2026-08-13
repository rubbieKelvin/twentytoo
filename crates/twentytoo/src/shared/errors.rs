//! The framework's HTTP error type.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use twentytoo_core::DataError;

use crate::shared::utils::escape_html;

/// A request-level failure, rendered as an HTML error response.
///
/// Policy denials surface as `Forbidden`; adapter failures map through
/// `Data` to their HTTP equivalents (`01` §10.3).
#[derive(Debug)]
pub enum AppError {
    /// 404 — the resource or record does not exist.
    NotFound,
    /// 403 — the actor may not perform this operation.
    Forbidden,
    /// 400 — malformed request (bad params, bad payload).
    BadRequest(String),
    /// 409 — optimistic-concurrency or uniqueness conflict.
    Conflict,
    /// 422 — the payload failed validation.
    Validation(String),
    /// Template render failure.
    Template(minijinja::Error),
    /// Data-layer failure.
    Data(DataError),
    /// Any other failure.
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound => return write!(f, "not found"),
            AppError::Forbidden => return write!(f, "forbidden"),
            AppError::BadRequest(msg) => return write!(f, "bad request: {msg}"),
            AppError::Conflict => return write!(f, "conflict"),
            AppError::Validation(msg) => return write!(f, "validation error: {msg}"),
            AppError::Template(e) => return write!(f, "template error: {e}"),
            AppError::Data(e) => return write!(f, "data error: {e}"),
            AppError::Internal(e) => return write!(f, "internal error: {e}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Template(e) => return Some(e),
            AppError::Data(e) => return Some(e),
            AppError::Internal(e) => return Some(e.as_ref()),
            _ => return None,
        }
    }
}

/// A boot-time failure: validation or template assembly.
///
/// Deliberately separate from [`AppError`] — nothing is being served.
#[derive(Debug)]
pub enum BuildError {
    /// A resource's identifiers failed validation against its source.
    Data(DataError),
    /// Template assembly failed (syntax, unreadable override dir, or a
    /// referenced name that does not resolve).
    Template(minijinja::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Data(e) => return write!(f, "boot validation failed: {e}"),
            BuildError::Template(e) => return write!(f, "template build failed: {e}"),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::Data(e) => return Some(e),
            BuildError::Template(e) => return Some(e),
        }
    }
}

impl From<DataError> for BuildError {
    fn from(e: DataError) -> Self {
        return BuildError::Data(e);
    }
}

impl From<minijinja::Error> for BuildError {
    fn from(e: minijinja::Error) -> Self {
        return BuildError::Template(e);
    }
}

impl From<DataError> for AppError {
    fn from(e: DataError) -> Self {
        return AppError::Data(e);
    }
}

impl From<minijinja::Error> for AppError {
    fn from(e: minijinja::Error) -> Self {
        return AppError::Template(e);
    }
}

impl From<axum::extract::rejection::RawFormRejection> for AppError {
    fn from(e: axum::extract::rejection::RawFormRejection) -> Self {
        return AppError::BadRequest(e.to_string());
    }
}

impl From<axum::extract::rejection::QueryRejection> for AppError {
    fn from(e: axum::extract::rejection::QueryRejection) -> Self {
        return AppError::BadRequest(e.to_string());
    }
}

impl From<axum::extract::rejection::PathRejection> for AppError {
    fn from(e: axum::extract::rejection::PathRejection) -> Self {
        return AppError::BadRequest(e.to_string());
    }
}

/// The status for a `DataError`, by category.
fn data_status(e: &DataError) -> StatusCode {
    match e {
        DataError::NotFound => return StatusCode::NOT_FOUND,
        DataError::Conflict => return StatusCode::CONFLICT,
        DataError::Validation(_) => return StatusCode::UNPROCESSABLE_ENTITY,
        DataError::Unauthorized => return StatusCode::BAD_GATEWAY,
        DataError::RateLimited => return StatusCode::TOO_MANY_REQUESTS,
        DataError::Unsupported => return StatusCode::NOT_IMPLEMENTED,
        DataError::Internal(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden".to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Conflict => (
                StatusCode::CONFLICT,
                "Conflict — the record changed; reload and retry".to_string(),
            ),
            AppError::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            AppError::Template(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {e}"),
            ),
            AppError::Data(e) => {
                let status = data_status(&e);
                let message = match &e {
                    DataError::NotFound => "Not found".to_string(),
                    DataError::Conflict => {
                        "Conflict — the record changed; reload and retry".to_string()
                    }
                    DataError::Validation(msg) => msg.clone(),
                    _ => e.to_string(),
                };
                (status, message)
            }
            AppError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {e}"),
            ),
        };
        let body = format!(
            "<!doctype html><html><body><h1>{}</h1><p>{}</p></body></html>",
            status.as_u16(),
            escape_html(&message)
        );
        return (status, axum::response::Html(body)).into_response();
    }
}
