//! What can go wrong setting up, upgrading or removing a rehearsal.

use crate::upgrade_server::SERVER_VARIABLE;
use andromeda_postgresql_context_store::error::ContextStoreError;
use sqlx::migrate::MigrateError;
use thiserror::Error;
use url::ParseError;

/// Why the server a rehearsal runs on could not be named.
#[derive(Debug, Error)]
pub enum UpgradeServerError {
    /// The environment variable is unset.
    #[error("the environment variable {} is not set", SERVER_VARIABLE)]
    Unset,
    /// The environment variable holds bytes that are not Unicode.
    #[error("the environment variable {} is not Unicode", SERVER_VARIABLE)]
    NotUnicode,
    /// The value is not a URL.
    #[error("the environment variable {} is not a URL", SERVER_VARIABLE)]
    Malformed(#[source] ParseError),
}

/// Why a rehearsal database could not be created, migrated, queried or dropped.
#[derive(Debug, Error)]
pub enum UpgradeDatabaseError {
    /// The server is not named.
    #[error(transparent)]
    Server(#[from] UpgradeServerError),
    /// The broker's database code refused the step.
    #[error("the broker's database code failed")]
    Store(#[from] ContextStoreError),
    /// A migration of the release could not be applied.
    #[error("a migration of the release could not be applied")]
    ReleaseMigration(#[from] MigrateError),
    /// A connection to the database failed.
    #[error("the database could not be reached")]
    Connection(#[from] sqlx::Error),
    /// A session is still connected to the database, so it was kept.
    #[error("a session is still connected to the database")]
    StillConnected,
}
