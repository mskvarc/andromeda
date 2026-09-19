//! Exclusive global-state capability for tests that require an empty broker database.

use crate::{
    configuration::{ConfigurationError, TestRunConfiguration},
    database_isolation::{DatabaseIsolationError, SerializedLease, connect_pool},
    database_reset::{DatabaseResetError, reset_tenant},
};
use andromeda_test_client::test_client::TestClient;
use sqlx::PgPool;
use thiserror::Error;

/// Environment holding the exclusive cross-process global-state lock.
#[must_use = "hold the serialized environment for the complete global-state test and finish it explicitly when cleanup is required"]
pub struct SerializedEnvironment {
    client: TestClient,
    lease: SerializedLease,
    pool: PgPool,
}

impl SerializedEnvironment {
    /// Read process configuration, acquire exclusive ownership, and reset mutable tenant state.
    ///
    /// # Errors
    ///
    /// Returns [`SerializedEnvironmentError::Configuration`] when the process environment is invalid, [`SerializedEnvironmentError::Isolation`] when the pool or the exclusive advisory lock cannot be obtained, and [`SerializedEnvironmentError::Reset`] when the tenant reset fails.
    pub async fn from_environment_reset() -> Result<SerializedEnvironment, SerializedEnvironmentError> {
        let configuration = TestRunConfiguration::from_environment().map_err(SerializedEnvironmentError::Configuration)?;
        let mut environment = SerializedEnvironment::acquire(&configuration).await?;
        environment.reset().await?;
        Ok(environment)
    }

    /// Acquire exclusive serialized-state ownership.
    ///
    /// # Errors
    ///
    /// Returns [`SerializedEnvironmentError::Isolation`] when the pool cannot be opened or the exclusive advisory lock cannot be acquired.
    pub async fn acquire(configuration: &TestRunConfiguration) -> Result<SerializedEnvironment, SerializedEnvironmentError> {
        let pool = connect_pool(configuration.database()).await?;
        let lease = SerializedLease::acquire(&pool).await?;
        Ok(SerializedEnvironment {
            client: TestClient::new(configuration.broker().clone(), None), // clone: the environment owns its client endpoint while configuration remains reusable.
            lease,
            pool,
        })
    }

    /// Borrow the test client while exclusive state ownership is held.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Borrow the database pool for assertions that inspect broker persistence state.
    ///
    /// The exclusive lease serializes every holder, so a plan or catalogue query
    /// issued through this pool reads a tenant reset to a clean slate and holding
    /// only what the test itself seeded -- whole-table `ANALYZE` and unscoped
    /// `EXPLAIN` are therefore deterministic in a way the shared-lock environment
    /// cannot promise.
    pub const fn database_pool(&self) -> &PgPool {
        &self.pool
    }

    /// Reset all mutable tenant tables through the exclusive capability.
    ///
    /// # Errors
    ///
    /// Returns [`SerializedEnvironmentError::Reset`] when the exclusive lease no longer holds its connection or the reset script fails.
    pub async fn reset(&mut self) -> Result<(), SerializedEnvironmentError> {
        reset_tenant(&mut self.lease).await.map_err(SerializedEnvironmentError::Reset)
    }

    /// Release exclusive state ownership.
    ///
    /// # Errors
    ///
    /// Returns [`SerializedEnvironmentError::Isolation`] when the exclusive advisory lock cannot be released.
    pub async fn finish(self) -> Result<(), SerializedEnvironmentError> {
        let result = self.lease.release().await.map_err(SerializedEnvironmentError::Isolation);
        self.pool.close().await;
        result
    }

    /// Reset mutable tenant state and then release exclusive ownership.
    ///
    /// # Errors
    ///
    /// Returns the [`SerializedEnvironment::reset`] failure or the [`SerializedEnvironment::finish`] failure when exactly one of them fails, and [`SerializedEnvironmentError::ResetAndRelease`] when both do.
    pub async fn reset_and_finish(mut self) -> Result<(), SerializedEnvironmentError> {
        let reset = self.reset().await;
        let finish = self.finish().await;
        match (reset, finish) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(reset), Ok(())) => Err(reset),
            (Ok(()), Err(finish)) => Err(finish),
            (Err(reset), Err(finish)) => Err(SerializedEnvironmentError::ResetAndRelease {
                reset: Box::new(reset),
                release: Box::new(finish),
            }),
        }
    }
}

/// Serialized environment allocation, reset, or release failure.
#[derive(Debug, Error)]
pub enum SerializedEnvironmentError {
    /// Runtime test configuration is invalid.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Database connection or exclusive advisory-lock operation failed.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// Global reset failed.
    #[error(transparent)]
    Reset(#[from] DatabaseResetError),
    /// Both the final reset and release failed.
    #[error("serialized reset failed ({reset}) and release failed ({release})")]
    ResetAndRelease {
        /// Reset failure.
        reset: Box<SerializedEnvironmentError>,
        /// Release failure.
        release: Box<SerializedEnvironmentError>,
    },
}
