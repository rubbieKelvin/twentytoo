//! The database layer's error type.

use twentytoo_core::DataError;

/// A database-layer failure, mapped from `sqlx::Error`.
///
/// Categories mirror [`DataError`] so adapters can translate directly;
/// `Conflict` carries the constraint message for user-facing reporting.
#[derive(Debug)]
pub enum DbError {
    /// The row did not exist (update/delete on a missing id).
    NotFound,
    /// A unique constraint was violated: duplicate email, slug, code, key,
    /// or role grant.
    Conflict(String),
    /// Input failed validation before reaching the database (e.g. a
    /// malformed permission code).
    Validation(String),
    /// Any other database failure.
    Internal(sqlx::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::NotFound => return write!(f, "row not found"),
            DbError::Conflict(msg) => return write!(f, "conflict: {msg}"),
            DbError::Validation(msg) => return write!(f, "validation error: {msg}"),
            DbError::Internal(e) => return write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        return match self {
            DbError::Internal(e) => Some(e),
            _ => None,
        };
    }
}

/// PostgreSQL error code for a unique-constraint violation.
const UNIQUE_VIOLATION: &str = "23505";

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        if let Some(db) = e.as_database_error()
            && db.code().as_deref() == Some(UNIQUE_VIOLATION)
        {
            return DbError::Conflict(db.message().to_string());
        }
        return DbError::Internal(e);
    }
}

impl From<DbError> for DataError {
    fn from(e: DbError) -> Self {
        return match e {
            DbError::NotFound => DataError::NotFound,
            DbError::Conflict(_) => DataError::Conflict,
            DbError::Validation(msg) => DataError::Validation(msg),
            DbError::Internal(e) => DataError::Internal(Box::new(e)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_errors_stay_internal() {
        let e: DbError = sqlx::Error::RowNotFound.into();
        assert!(matches!(e, DbError::Internal(_)));
    }

    #[test]
    fn maps_into_core_data_error() {
        let e: DataError = DbError::Validation("bad code".to_string()).into();
        assert!(matches!(e, DataError::Validation(_)));
        let e: DataError = DbError::NotFound.into();
        assert!(matches!(e, DataError::NotFound));
    }
}
