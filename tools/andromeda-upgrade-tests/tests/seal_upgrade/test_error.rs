//! The failures a rehearsal test propagates instead of unwrapping.

use andromeda_upgrade_tests::upgrade_database_error::{UpgradeDatabaseError, UpgradeServerError};
use sea_query::error::Error as StatementError;
use thiserror::Error;

/// Why a rehearsal test could not run to its assertions.
#[derive(Debug, Error)]
pub(crate) enum UpgradeTestError {
    /// The database could not be created, migrated or dropped.
    #[error(transparent)]
    Database(#[from] UpgradeDatabaseError),
    /// The server is not named.
    #[error(transparent)]
    Server(#[from] UpgradeServerError),
    /// A statement failed on the server.
    #[error("a statement failed")]
    Query(#[from] sqlx::Error),
    /// A statement could not be assembled.
    #[error("a statement could not be assembled")]
    Statement(#[from] StatementError),
    /// A row the test needs is missing.
    #[error("the row for {what} is missing")]
    Missing {
        /// What the test looked for.
        what: &'static str,
    },
}
