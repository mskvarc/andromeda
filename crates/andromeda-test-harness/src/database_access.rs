//! Explicit test-database access and serialized reset entry points.

use crate::{
    configuration::TestRunConfiguration,
    database_isolation::{DatabaseIsolationError, connect_pool},
};
use sqlx::PgPool;

/// Connect to the configured test database for persistence assertions.
pub async fn connect_test_database() -> Result<PgPool, DatabaseAccessError> {
    let configuration = TestRunConfiguration::from_environment().map_err(DatabaseAccessError::Configuration)?;
    connect_pool(configuration.database()).await.map_err(DatabaseAccessError::Isolation)
}

/// Test database configuration or connection failure.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseAccessError {
    /// Runtime configuration is invalid.
    #[error(transparent)]
    Configuration(#[from] crate::configuration::ConfigurationError),
    /// Database connection or advisory-lock acquisition failed.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
}
