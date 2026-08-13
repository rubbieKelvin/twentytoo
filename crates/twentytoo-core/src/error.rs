//! The data-layer error contract shared by every adapter.

/// An adapter-level failure.
///
/// `Unauthorized` means the *source's* own credentials failed, not a policy
/// denial — policy denials never reach the adapter. `Unsupported` is a
/// defensive backstop for engine bugs; capabilities are the primary signaling
/// mechanism.
#[derive(Debug)]
pub enum DataError {
    /// The record does not exist.
    NotFound,
    /// Optimistic concurrency / unique violation.
    Conflict,
    /// The payload failed validation.
    Validation(String),
    /// The source's credentials failed.
    Unauthorized,
    /// The source rate-limited the request.
    RateLimited,
    /// The source cannot express this operation (capability violation).
    Unsupported,
    /// Any other failure.
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataError::NotFound => return write!(f, "record not found"),
            DataError::Conflict => return write!(f, "conflict"),
            DataError::Validation(msg) => return write!(f, "validation error: {msg}"),
            DataError::Unauthorized => return write!(f, "unauthorized"),
            DataError::RateLimited => return write!(f, "rate limited"),
            DataError::Unsupported => return write!(f, "unsupported"),
            DataError::Internal(e) => return write!(f, "internal error: {e}"),
        }
    }
}

impl std::error::Error for DataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DataError::Internal(e) => return Some(e.as_ref()),
            _ => return None,
        }
    }
}
