//! How a failure met while changing a tenant's schema is classified.
//!
//! Each PostgreSQL migration runs in its own transaction together with its
//! ledger row, so a migration cut short by a lost connection, a saturated pool
//! or a deadlock rolls back whole and the next run resumes it. Such a failure
//! keeps the retryable class [`classify`] gives it. Every other cause is the
//! schema itself being wrong, whatever class the statement alone would have:
//! a migration refused for a unique violation is a schema fault, not a client
//! conflict.

use crate::persistence_classification::classify;
use andromeda_persistence_failure::{persistence_failure::PersistenceFailure, retryability::Retryability};
use sqlx::migrate::MigrateError;
use std::error::Error as StdError;

/// The failure class of a driver error met while changing a schema.
///
/// A retryable class (saturated, rolled back, unavailable) is kept, because the
/// schema change rolled back and can run again unchanged. Every other class is
/// [`PersistenceFailure::SchemaMismatch`].
#[must_use]
pub fn classify_schema_change(error: &sqlx::Error) -> PersistenceFailure {
    let failure = classify(error);
    match failure.retryability() {
        Retryability::Retryable(_) => failure,
        Retryability::Fault => PersistenceFailure::SchemaMismatch,
    }
}

/// The failure class of a migration run, by its driver cause.
///
/// A migration failure whose source is a driver error, a statement the
/// database could not run, is classified by [`classify_schema_change`]. One
/// with no driver cause (a migration history the embedded migrations do not
/// match, an unreadable migration source, a driver capability the run lacks)
/// is [`PersistenceFailure::SchemaMismatch`].
#[must_use]
pub fn classify_migration(error: &MigrateError) -> PersistenceFailure {
    error
        .source()
        .and_then(|cause| cause.downcast_ref::<sqlx::Error>())
        .map_or(PersistenceFailure::SchemaMismatch, classify_schema_change)
}

#[cfg(test)]
mod tests {
    use crate::{
        fake_database_error::FakeDatabaseError,
        schema_change_classification::{classify_migration, classify_schema_change},
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;
    use sqlx::migrate::MigrateError;
    use std::io::{Error as IoError, ErrorKind};

    /// A connection the server reset mid-statement.
    fn connection_reset() -> sqlx::Error {
        sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))
    }

    #[test]
    fn an_interrupted_migration_is_the_store_being_unavailable() {
        assert_eq!(classify_migration(&MigrateError::Execute(connection_reset())), PersistenceFailure::Unavailable);
        assert_eq!(
            classify_migration(&MigrateError::ExecuteMigration(
                FakeDatabaseError::new("terminating connection due to administrator command", "57P01").into_sqlx(),
                30
            )),
            PersistenceFailure::Unavailable
        );
        assert_eq!(
            classify_migration(&MigrateError::Execute(FakeDatabaseError::new("connection failure", "08006").into_sqlx())),
            PersistenceFailure::Unavailable
        );
    }

    #[test]
    fn a_migration_starved_of_a_connection_is_saturated() {
        assert_eq!(
            classify_migration(&MigrateError::Execute(sqlx::Error::PoolTimedOut)),
            PersistenceFailure::Saturated
        );
    }

    #[test]
    fn a_migration_history_the_binary_does_not_match_is_a_schema_mismatch() {
        for error in [
            MigrateError::VersionMismatch(30),
            MigrateError::VersionMissing(30),
            MigrateError::VersionTooNew(31, 30),
            MigrateError::Dirty(30),
        ] {
            assert_eq!(classify_migration(&error), PersistenceFailure::SchemaMismatch, "{error}");
        }
    }

    #[test]
    fn a_migration_statement_the_database_refuses_is_a_schema_mismatch() {
        assert_eq!(
            classify_migration(&MigrateError::ExecuteMigration(
                FakeDatabaseError::new("relation already exists", "42P07").into_sqlx(),
                30
            )),
            PersistenceFailure::SchemaMismatch
        );
        assert_eq!(
            classify_migration(&MigrateError::ExecuteMigration(FakeDatabaseError::new("unique", "23505").into_sqlx(), 30)),
            PersistenceFailure::SchemaMismatch
        );
    }

    #[test]
    fn a_schema_change_fault_is_a_schema_mismatch_and_an_outage_stays_retryable() {
        assert_eq!(classify_schema_change(&connection_reset()), PersistenceFailure::Unavailable);
        assert_eq!(
            classify_schema_change(&FakeDatabaseError::new("deadlock", "40P01").into_sqlx()),
            PersistenceFailure::RolledBack
        );
        assert_eq!(
            classify_schema_change(&FakeDatabaseError::new("undefined table", "42P01").into_sqlx()),
            PersistenceFailure::SchemaMismatch
        );
        assert_eq!(
            classify_schema_change(&FakeDatabaseError::new("invalid text", "22P02").into_sqlx()),
            PersistenceFailure::SchemaMismatch
        );
    }
}
