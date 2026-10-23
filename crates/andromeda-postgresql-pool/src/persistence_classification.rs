//! How a `sqlx` error is classified into the backend-neutral failure a port
//! carries.

use crate::{
    saturation::is_pool_saturated,
    schema_change_classification::classify_migration,
    sql_state::SqlState,
    store_unavailability::is_store_unavailable,
    transaction_rollback::is_transaction_rolled_back,
};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use sqlx::error::DatabaseError;

/// Classify a `sqlx` error and record it for operators.
///
/// The driver error is logged at the caller's location by
/// [`PersistenceFailure::record`]; only the classification travels on through
/// the port, so no driver detail can reach a client.
#[track_caller]
#[must_use]
pub fn persistence_failure(error: &sqlx::Error) -> PersistenceFailure {
    classify(error).record(error)
}

/// The failure class of a `sqlx` error, without recording it.
///
/// The retryable conditions are recognised first, in this order: a saturated
/// pool or pooler, a deadlock or serialization victim, and a store that cannot
/// be reached. None of them is ever reported as a fault. A migration error is
/// classified by its cause, as a schema change.
#[must_use]
pub fn classify(error: &sqlx::Error) -> PersistenceFailure {
    if is_pool_saturated(error) {
        return PersistenceFailure::Saturated;
    }
    if is_transaction_rolled_back(error) {
        return PersistenceFailure::RolledBack;
    }
    if is_store_unavailable(error) {
        return PersistenceFailure::Unavailable;
    }
    match error {
        sqlx::Error::Database(database) => classify_database_error(database.as_ref()),
        // Every shape these take is recognised above; they are listed so that
        // a shape the predicates miss is still the store being unreachable.
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => PersistenceFailure::Unavailable,
        sqlx::Error::Configuration(_) | sqlx::Error::ConfigFile(_) => PersistenceFailure::Misconfigured,
        sqlx::Error::RowNotFound => PersistenceFailure::RecordMissing,
        sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::AnyDriverError(_) => PersistenceFailure::Undecodable,
        sqlx::Error::Encode(_) => PersistenceFailure::Unencodable,
        sqlx::Error::InvalidArgument(_) | sqlx::Error::InvalidSavePointStatement => PersistenceFailure::StatementUnbuildable,
        sqlx::Error::Migrate(error) => classify_migration(error),
        // `sqlx::Error` is `#[non_exhaustive]`: a variant added upstream is a
        // failure of the statement until it is classified here.
        _unclassified => PersistenceFailure::StatementRejected,
    }
}

/// The failure class of an error the database itself reported, by SQLSTATE
/// class: `23` integrity constraint violations, `42` syntax or access-rule
/// violations (an undefined relation or column is a schema drift), `22` data
/// exceptions (a value the database cannot take). Connection exceptions are
/// the store being unreachable and are decided before this.
///
/// An invalid regular expression (`2201B`, in class `22`) is a pattern the
/// statement carries, not a value the broker failed to encode: it is a
/// rejected statement on every path that runs a `RegExp` filter.
fn classify_database_error(error: &dyn DatabaseError) -> PersistenceFailure {
    if SqlState::of_database(error) == Some(SqlState::InvalidRegularExpression) {
        return PersistenceFailure::StatementRejected;
    }
    let code = error.code();
    match code.as_deref().and_then(|code| code.get(..2)) {
        Some("23") => PersistenceFailure::ConstraintViolated,
        Some("42") => PersistenceFailure::SchemaMismatch,
        Some("22") => PersistenceFailure::Unencodable,
        Some(_) | None => PersistenceFailure::StatementRejected,
    }
}

#[cfg(test)]
mod tests {
    use crate::{fake_database_error::FakeDatabaseError, persistence_classification::classify};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;
    use sqlx::migrate::MigrateError;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn a_pool_timeout_and_a_deadlock_are_backpressure() {
        assert_eq!(classify(&sqlx::Error::PoolTimedOut), PersistenceFailure::Saturated);
        assert_eq!(
            classify(&FakeDatabaseError::new("deadlock", "40P01").into_sqlx()),
            PersistenceFailure::RolledBack
        );
        assert_eq!(
            classify(&FakeDatabaseError::new("serialization", "40001").into_sqlx()),
            PersistenceFailure::RolledBack
        );
    }

    #[test]
    fn a_transport_fault_is_the_store_being_unavailable() {
        assert_eq!(
            classify(&sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))),
            PersistenceFailure::Unavailable
        );
        assert_eq!(classify(&sqlx::Error::PoolClosed), PersistenceFailure::Unavailable);
        assert_eq!(classify(&sqlx::Error::WorkerCrashed), PersistenceFailure::Unavailable);
        assert_eq!(
            classify(&FakeDatabaseError::new("connection failure", "08006").into_sqlx()),
            PersistenceFailure::Unavailable
        );
    }

    #[test]
    fn a_server_going_away_classifies_as_unavailable() {
        assert_eq!(
            classify(&FakeDatabaseError::new("terminating connection due to administrator command", "57P01").into_sqlx()),
            PersistenceFailure::Unavailable
        );
    }

    #[test]
    fn a_pooler_login_failure_classifies_as_unavailable() {
        assert_eq!(
            classify(&FakeDatabaseError::new("server login has been failing, try again later (server_login_retry)", "08P01").into_sqlx()),
            PersistenceFailure::Unavailable
        );
        assert_eq!(
            classify(&FakeDatabaseError::new("query_wait_timeout", "08P01").into_sqlx()),
            PersistenceFailure::Saturated
        );
    }

    #[test]
    fn connection_configuration_errors_are_misconfiguration() {
        assert_eq!(classify(&sqlx::Error::Configuration("invalid port".into())), PersistenceFailure::Misconfigured);
    }

    #[test]
    fn a_missing_row_is_a_missing_record() {
        assert_eq!(classify(&sqlx::Error::RowNotFound), PersistenceFailure::RecordMissing);
    }

    #[test]
    fn a_column_the_row_does_not_decode_is_undecodable() {
        assert_eq!(classify(&sqlx::Error::ColumnNotFound("missing".to_owned())), PersistenceFailure::Undecodable);
        assert_eq!(
            classify(&sqlx::Error::ColumnIndexOutOfBounds { index: 3, len: 2 }),
            PersistenceFailure::Undecodable
        );
    }

    #[test]
    fn database_errors_are_classified_by_their_sqlstate_class() {
        assert_eq!(
            classify(&FakeDatabaseError::new("unique", "23505").into_sqlx()),
            PersistenceFailure::ConstraintViolated
        );
        assert_eq!(
            classify(&FakeDatabaseError::new("undefined table", "42P01").into_sqlx()),
            PersistenceFailure::SchemaMismatch
        );
        assert_eq!(
            classify(&FakeDatabaseError::new("invalid text", "22P02").into_sqlx()),
            PersistenceFailure::Unencodable
        );
        assert_eq!(
            classify(&FakeDatabaseError::new("statement timeout", "57014").into_sqlx()),
            PersistenceFailure::StatementRejected
        );
    }

    #[test]
    fn an_invalid_regular_expression_is_a_rejected_statement_not_an_unencodable_value() {
        assert_eq!(
            classify(&FakeDatabaseError::new("invalid regular expression", "2201B").into_sqlx()),
            PersistenceFailure::StatementRejected
        );
    }

    #[test]
    fn a_wrapped_migration_error_is_classified_by_its_cause() {
        assert_eq!(
            classify(&sqlx::Error::Migrate(Box::new(MigrateError::Execute(sqlx::Error::Io(IoError::from(
                ErrorKind::ConnectionReset
            )))))),
            PersistenceFailure::Unavailable
        );
        assert_eq!(
            classify(&sqlx::Error::Migrate(Box::new(MigrateError::VersionMismatch(1)))),
            PersistenceFailure::SchemaMismatch
        );
    }

    #[test]
    fn an_invalid_argument_is_a_statement_that_could_not_be_built() {
        assert_eq!(
            classify(&sqlx::Error::InvalidArgument("bind".to_owned())),
            PersistenceFailure::StatementUnbuildable
        );
    }
}
