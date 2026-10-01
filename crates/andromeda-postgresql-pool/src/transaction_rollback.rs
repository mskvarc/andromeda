//! Whether a driver error is a transaction rolled back under contention.

use sqlx::error::DatabaseError;

/// Whether a `sqlx` error is a transient transaction rollback that re-running
/// the transaction can clear.
///
/// PostgreSQL rolls the whole transaction back on a deadlock (SQLSTATE 40P01) or
/// a serialization failure (40001) and reports it to whichever transaction it
/// chose as the victim, so the operation is safely retryable from a fresh
/// transaction rather than a fault. The caller owns the bounded retry budget.
#[must_use]
pub fn is_transaction_rolled_back(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(DatabaseError::code)
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01"))
}

#[cfg(test)]
mod tests {
    use crate::{fake_database_error::FakeDatabaseError, transaction_rollback::is_transaction_rolled_back};

    /// A database error reporting the given SQLSTATE.
    fn database_error(code: &str) -> sqlx::Error {
        FakeDatabaseError::new("contended", code).into_sqlx()
    }

    #[test]
    fn a_deadlock_or_serialization_failure_is_a_transaction_rollback() {
        // PostgreSQL rolls the victim transaction back whole on a deadlock (40P01)
        // or a serialization failure (40001), so re-running it can clear the race.
        assert!(is_transaction_rolled_back(&database_error("40P01")));
        assert!(is_transaction_rolled_back(&database_error("40001")));
    }

    #[test]
    fn an_unrelated_database_error_or_transport_fault_is_not_a_transaction_rollback() {
        assert!(!is_transaction_rolled_back(&database_error("23505")));
        assert!(!is_transaction_rolled_back(&sqlx::Error::PoolTimedOut));
        assert!(!is_transaction_rolled_back(&sqlx::Error::RowNotFound));
    }
}
