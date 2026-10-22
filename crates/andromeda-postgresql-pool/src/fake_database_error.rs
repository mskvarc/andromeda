//! A database error the classification tests can construct.
//!
//! A real `PgDatabaseError` cannot be built without a live driver connection,
//! so the tests stand in this minimal [`DatabaseError`] carrying a fixed message,
//! SQLSTATE and, optionally, constraint name.

use sqlx::error::{DatabaseError, ErrorKind as SqlxErrorKind};
use std::{borrow::Cow, error::Error as StdError, fmt};

/// A minimal [`DatabaseError`] carrying a fixed message and SQLSTATE.
#[derive(Debug)]
pub struct FakeDatabaseError {
    /// The error message the classifier inspects.
    message: String,
    /// The SQLSTATE the error reports.
    code: String,
    /// The constraint the error reports as violated, if any.
    constraint: Option<String>,
}

impl FakeDatabaseError {
    /// A database error reporting `code` with `message`.
    #[must_use]
    pub fn new(message: &str, code: &str) -> FakeDatabaseError {
        FakeDatabaseError {
            message: message.to_string(),
            code: code.to_string(),
            constraint: None,
        }
    }

    /// The same error reporting `constraint` as the violated constraint.
    #[must_use]
    pub fn with_constraint(self, constraint: &str) -> FakeDatabaseError {
        FakeDatabaseError {
            constraint: Some(constraint.to_string()),
            ..self
        }
    }

    /// Wrap the error the way the driver hands a database error back.
    #[must_use]
    pub fn into_sqlx(self) -> sqlx::Error {
        sqlx::Error::Database(Box::new(self))
    }
}

// Written by hand: the driver's `DatabaseError` trait requires `Display`, and
// the rendering mirrors how PostgreSQL reports a code alongside its message.
impl fmt::Display for FakeDatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl StdError for FakeDatabaseError {}

impl DatabaseError for FakeDatabaseError {
    fn message(&self) -> &str {
        &self.message
    }

    fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.code))
    }

    fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }

    fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self
    }

    fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
        self
    }

    fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
        self
    }

    fn kind(&self) -> SqlxErrorKind {
        SqlxErrorKind::Other
    }
}
