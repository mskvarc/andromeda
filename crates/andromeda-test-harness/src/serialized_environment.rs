//! Exclusive global-state capability for tests that require an empty broker database.

use crate::{
    configuration::TestRunConfiguration,
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
    pub async fn from_environment_reset() -> Result<SerializedEnvironment, SerializedEnvironmentError> {
        let configuration = TestRunConfiguration::from_environment().map_err(SerializedEnvironmentError::Configuration)?;
        let mut environment = SerializedEnvironment::acquire(&configuration).await?;
        environment.reset().await?;
        Ok(environment)
    }

    /// Acquire exclusive serialized-state ownership.
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

    /// Reset all mutable tenant tables through the exclusive capability.
    pub async fn reset(&mut self) -> Result<(), SerializedEnvironmentError> {
        reset_tenant(&mut self.lease).await.map_err(SerializedEnvironmentError::Reset)
    }

    /// Release exclusive state ownership.
    pub async fn finish(self) -> Result<(), SerializedEnvironmentError> {
        let result = self.lease.release().await.map_err(SerializedEnvironmentError::Isolation);
        self.pool.close().await;
        result
    }

    /// Reset mutable tenant state and then release exclusive ownership.
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
    Configuration(#[from] crate::configuration::ConfigurationError),
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
