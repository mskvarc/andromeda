//! The SQLSTATE codes the broker tells apart.
//!
//! PostgreSQL reports a failed statement under a five-character SQLSTATE. The
//! broker names each code it reacts to once, here, so no store compares a
//! driver error against a string literal. Codes the broker does not react to
//! are `None` from [`SqlState::of`]; classes of codes (connection exceptions
//! `08`, data exceptions `22`, integrity violations `23`, syntax and access
//! rule violations `42`) are matched on the code's prefix where they matter.

use sqlx::error::DatabaseError;
use strum::{EnumString, IntoStaticStr};

/// A SQLSTATE the broker reacts to.
#[derive(Clone, Copy, Debug, Eq, EnumString, IntoStaticStr, PartialEq)]
pub enum SqlState {
    /// `3D000`, `invalid_catalog_name`: the database does not exist.
    #[strum(serialize = "3D000")]
    InvalidCatalogName,
    /// `3F000`, `invalid_schema_name`: the schema does not exist.
    #[strum(serialize = "3F000")]
    InvalidSchemaName,
    /// `22023`, `invalid_parameter_value`: a parameter the database refuses,
    /// which for an ICU collation identifier is the client's own value.
    #[strum(serialize = "22023")]
    InvalidParameterValue,
    /// `2201B`, `invalid_regular_expression`: a pattern the database cannot
    /// compile.
    #[strum(serialize = "2201B")]
    InvalidRegularExpression,
    /// `23505`, `unique_violation`.
    #[strum(serialize = "23505")]
    UniqueViolation,
    /// `40001`, `serialization_failure`: the transaction was rolled back to
    /// preserve its isolation level.
    #[strum(serialize = "40001")]
    SerializationFailure,
    /// `40P01`, `deadlock_detected`: the transaction was chosen as the
    /// deadlock victim and rolled back.
    #[strum(serialize = "40P01")]
    DeadlockDetected,
    /// `42501`, `insufficient_privilege`.
    #[strum(serialize = "42501")]
    InsufficientPrivilege,
    /// `42P01`, `undefined_table`: the relation does not exist.
    #[strum(serialize = "42P01")]
    UndefinedTable,
    /// `42P04`, `duplicate_database`: the database already exists.
    #[strum(serialize = "42P04")]
    DuplicateDatabase,
    /// `53300`, `too_many_connections`: the server is at its connection limit.
    #[strum(serialize = "53300")]
    TooManyConnections,
    /// `55006`, `object_in_use`: the object, such as a template database, is
    /// being used by another session.
    #[strum(serialize = "55006")]
    ObjectInUse,
    /// `55P03`, `lock_not_available`: a lock was not granted within
    /// `lock_timeout`.
    #[strum(serialize = "55P03")]
    LockNotAvailable,
    /// `57P01`, `admin_shutdown`: the backend was terminated by an
    /// administrator.
    #[strum(serialize = "57P01")]
    AdminShutdown,
    /// `57P02`, `crash_shutdown`: another backend crashed and the server is
    /// resetting.
    #[strum(serialize = "57P02")]
    CrashShutdown,
    /// `57P03`, `cannot_connect_now`: the server is starting up or shutting
    /// down.
    #[strum(serialize = "57P03")]
    CannotConnectNow,
}

impl SqlState {
    /// The SQLSTATE of a driver error, or `None` for an error the database did
    /// not report or a code the broker does not name.
    #[must_use]
    pub fn of(error: &sqlx::Error) -> Option<SqlState> {
        error.as_database_error().and_then(SqlState::of_database)
    }

    /// The SQLSTATE a database error reports, or `None` for a code the broker
    /// does not name.
    #[must_use]
    pub fn of_database(error: &dyn DatabaseError) -> Option<SqlState> {
        error.code().and_then(|code| code.parse().ok())
    }
}

/// Whether `error` is a unique violation of the named constraint.
///
/// A relation can carry more than one unique constraint (its surrogate key
/// beside the identifier a client chose) and only a violation of the
/// identifier's own constraint is the conflict the client caused. A caller
/// that reports a create as already existing therefore checks the constraint,
/// not the SQLSTATE alone.
#[must_use]
pub fn violates_unique_constraint(error: &sqlx::Error, constraint: &str) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| SqlState::of_database(database) == Some(SqlState::UniqueViolation) && database.constraint() == Some(constraint))
}

#[cfg(test)]
mod tests {
    use crate::{
        fake_database_error::FakeDatabaseError,
        sql_state::{SqlState, violates_unique_constraint},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn each_named_code_parses_to_its_variant_and_back() {
        for (code, state) in [
            ("3D000", SqlState::InvalidCatalogName),
            ("3F000", SqlState::InvalidSchemaName),
            ("22023", SqlState::InvalidParameterValue),
            ("2201B", SqlState::InvalidRegularExpression),
            ("23505", SqlState::UniqueViolation),
            ("40001", SqlState::SerializationFailure),
            ("40P01", SqlState::DeadlockDetected),
            ("42501", SqlState::InsufficientPrivilege),
            ("42P01", SqlState::UndefinedTable),
            ("42P04", SqlState::DuplicateDatabase),
            ("53300", SqlState::TooManyConnections),
            ("55006", SqlState::ObjectInUse),
            ("55P03", SqlState::LockNotAvailable),
            ("57P01", SqlState::AdminShutdown),
            ("57P02", SqlState::CrashShutdown),
            ("57P03", SqlState::CannotConnectNow),
        ] {
            assert_eq!(code.parse::<SqlState>().ok(), Some(state), "{code}");
            assert_eq!(<&'static str>::from(state), code);
        }
    }

    #[test]
    fn an_unnamed_code_or_a_non_database_error_has_no_state() {
        assert_eq!(SqlState::of(&FakeDatabaseError::new("syntax", "42601").into_sqlx()), None);
        assert_eq!(SqlState::of(&sqlx::Error::RowNotFound), None);
    }

    #[test]
    fn a_database_error_reports_its_state() {
        assert_eq!(
            SqlState::of(&FakeDatabaseError::new("deadlock", "40P01").into_sqlx()),
            Some(SqlState::DeadlockDetected)
        );
    }

    #[test]
    fn a_unique_violation_matches_only_the_constraint_it_names() {
        let violation = |code: &str, constraint: Option<&str>| {
            let error = FakeDatabaseError::new("violation", code);
            match constraint {
                Some(constraint) => error.with_constraint(constraint).into_sqlx(),
                None => error.into_sqlx(),
            }
        };
        assert!(violates_unique_constraint(
            &violation("23505", Some("uq_subscriptions_id")),
            "uq_subscriptions_id"
        ));
        assert!(!violates_unique_constraint(
            &violation("23505", Some("pk_subscriptions")),
            "uq_subscriptions_id"
        ));
        assert!(!violates_unique_constraint(&violation("23505", None), "uq_subscriptions_id"));
        assert!(!violates_unique_constraint(
            &violation("23514", Some("uq_subscriptions_id")),
            "uq_subscriptions_id"
        ));
    }

    #[test]
    fn a_failure_that_is_no_database_error_violates_no_constraint() {
        assert!(!violates_unique_constraint(&sqlx::Error::RowNotFound, "uq_subscriptions_id"));
    }
}
