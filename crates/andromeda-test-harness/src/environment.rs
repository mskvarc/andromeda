//! One isolated test execution context and its explicit cleanup lifecycle.

use crate::{
    configuration::{ConfigurationError, TestRunConfiguration},
    database_isolation::{DatabaseIsolationError, IsolatedLease, connect_pool},
    database_reset::{DatabaseResetError, cleanup_prefix},
    identifiers::{IdentifierError, TestIdentifiers, TestSuiteName},
    readiness::{ReadinessError, ReadinessPolicy, wait_until_ready_once},
};
use andromeda_test_client::test_client::TestClient;
use sqlx::PgPool;
use thiserror::Error;

/// Prefix-isolated integration-test environment.
pub struct TestEnvironment {
    client: TestClient,
    identifiers: TestIdentifiers,
    lease: IsolatedLease,
    pool: PgPool,
}

impl TestEnvironment {
    /// Build an isolated environment from process configuration for a named suite.
    ///
    /// # Errors
    ///
    /// Returns [`TestEnvironmentError::Configuration`] when the process environment is invalid, [`TestEnvironmentError::Identifier`] when the suite name is not a valid token, and every failure [`TestEnvironment::isolated`] reports.
    pub async fn from_suite(suite: &str) -> Result<TestEnvironment, TestEnvironmentError> {
        let configuration = TestRunConfiguration::from_environment().map_err(TestEnvironmentError::Configuration)?;
        let suite = TestSuiteName::parse(suite.to_owned())?;
        TestEnvironment::isolated(&configuration, &suite).await
    }

    /// Allocate identifiers, acquire a shared cross-process lease, and construct a probe-free client.
    ///
    /// # Errors
    ///
    /// Returns [`TestEnvironmentError::Identifier`] when identifier allocation fails, [`TestEnvironmentError::Readiness`] when the broker does not answer its health endpoint within the default policy, and [`TestEnvironmentError::Isolation`] when the pool or the shared advisory lock cannot be obtained.
    pub async fn isolated(configuration: &TestRunConfiguration, suite: &TestSuiteName) -> Result<TestEnvironment, TestEnvironmentError> {
        let identifiers = TestIdentifiers::allocate(suite)?;
        let client = TestClient::new(configuration.broker().clone(), None); // clone: the environment owns its client endpoint while configuration remains reusable.
        wait_until_ready_once(&client, ReadinessPolicy::default()).await?;
        let pool = connect_pool(configuration.database()).await?;
        let lease = IsolatedLease::acquire(&pool).await?;
        Ok(TestEnvironment {
            client,
            identifiers,
            lease,
            pool,
        })
    }

    /// Borrow the HTTP client bound to this test environment.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Borrow the structured identifier allocator.
    pub const fn identifiers(&self) -> &TestIdentifiers {
        &self.identifiers
    }

    /// Borrow the database pool for assertions that inspect broker persistence state.
    pub const fn database_pool(&self) -> &PgPool {
        &self.pool
    }

    /// Compose a wire identifier below this environment's non-empty isolated prefix.
    pub fn id(&self, local: &str) -> String {
        format!("{}:{local}", self.identifiers.prefix())
    }

    /// Borrow the non-empty cleanup prefix as wire text.
    pub fn id_prefix(&self) -> &str {
        self.identifiers.prefix().as_str()
    }

    /// Clean owned rows, release the advisory lock, and preserve both failures when necessary.
    ///
    /// # Errors
    ///
    /// Returns [`TestEnvironmentError::Cleanup`] when prefix cleanup fails, [`TestEnvironmentError::Isolation`] when the shared advisory lock cannot be released, and [`TestEnvironmentError::CleanupAndRelease`] when both fail.
    pub async fn finish(self) -> Result<(), TestEnvironmentError> {
        let cleanup = cleanup_prefix(&self.pool, &self.identifiers).await;
        let release = self.lease.release().await;
        self.pool.close().await;
        match (cleanup, release) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(cleanup), Ok(())) => Err(TestEnvironmentError::Cleanup(cleanup)),
            (Ok(()), Err(release)) => Err(TestEnvironmentError::Isolation(release)),
            (Err(cleanup), Err(release)) => Err(TestEnvironmentError::CleanupAndRelease { cleanup, release }),
        }
    }
}

/// Isolated environment allocation or cleanup failure.
#[derive(Debug, Error)]
pub enum TestEnvironmentError {
    /// Runtime test configuration is invalid.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Database connection or advisory-lock operation failed.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// Identifier allocation failed.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// The broker did not become ready within the configured policy.
    #[error(transparent)]
    Readiness(#[from] ReadinessError),
    /// Prefix cleanup failed.
    #[error(transparent)]
    Cleanup(#[from] DatabaseResetError),
    /// Both cleanup and advisory-lock release failed.
    #[error("cleanup failed ({cleanup}) and advisory-lock release failed ({release})")]
    CleanupAndRelease {
        /// Prefix cleanup failure.
        cleanup: DatabaseResetError,
        /// Lock release failure.
        release: DatabaseIsolationError,
    },
}
