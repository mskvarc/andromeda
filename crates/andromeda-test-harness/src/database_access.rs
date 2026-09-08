//! Explicit test-database access and serialized reset entry points.

use crate::{
    configuration::{ConfigurationError, TestRunConfiguration},
    database_isolation::{DatabaseIsolationError, connect_pool},
};
use sqlx::PgPool;

/// Connect to the configured test database for persistence assertions.
///
/// # Errors
///
/// Returns [`DatabaseAccessError::Configuration`] when the process environment does not describe a usable test database, and [`DatabaseAccessError::Isolation`] when the connection pool cannot be opened.
pub async fn connect_test_database() -> Result<PgPool, DatabaseAccessError> {
    let configuration = TestRunConfiguration::from_environment().map_err(DatabaseAccessError::Configuration)?;
    connect_pool(configuration.database()).await.map_err(DatabaseAccessError::Isolation)
}

/// Test database configuration or connection failure.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseAccessError {
    /// Runtime configuration is invalid.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Database connection or advisory-lock acquisition failed.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
}
