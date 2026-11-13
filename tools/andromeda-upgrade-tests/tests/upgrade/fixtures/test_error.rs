//! The failures a rehearsal test propagates instead of unwrapping.

use andromeda_context_space::snapshot_key::SnapshotKeyError;
use andromeda_ngsi_ld_lexicon::identifier::IdentifierParseError;
use andromeda_postgresql_attribute_instance::stored_members::StoredInstanceError;
use andromeda_snapshot::error::SnapshotError;
use andromeda_upgrade_tests::upgrade_database_error::{UpgradeDatabaseError, UpgradeServerError};
use sea_query::error::Error as StatementError;
use sqlx::migrate::MigrateError;
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
    /// A migration ledger could not be read.
    #[error("a migration ledger could not be read")]
    Ledger(#[from] MigrateError),
    /// A Snapshot key the database issued is not one.
    #[error("a snapshot key could not be read")]
    SnapshotKey(#[from] SnapshotKeyError),
    /// A Snapshot URI the test names is not one.
    #[error("a snapshot URI could not be read")]
    Identifier(#[from] IdentifierParseError),
    /// The broker's Snapshot store refused a step.
    #[error("the snapshot store failed")]
    Snapshot(#[from] SnapshotError),
    /// The broker's decoder refused an element the upgrade left.
    #[error("the broker cannot read a stored sub-attribute")]
    Stored(#[from] StoredInstanceError),
    /// A row the test needs is missing.
    #[error("the row for {what} is missing")]
    Missing {
        /// What the test looked for.
        what: &'static str,
    },
}
