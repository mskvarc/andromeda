//! Exclusive tenant-lifecycle environment.

use crate::{
    configuration::TestRunConfiguration,
    database_isolation::{DatabaseIsolationError, SerializedLease, connect_pool},
};
use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient, transport_error::TenantNameError};
use sqlx::PgPool;
use thiserror::Error;

/// Tenant lifecycle test environment holding the exclusive cross-process lock.
pub struct TenantEnvironment {
    client: TestClient,
    lease: SerializedLease,
    pool: PgPool,
}

impl TenantEnvironment {
    /// Acquire exclusive tenant-state ownership and construct a tenant-bound client.
    pub async fn acquire(configuration: &TestRunConfiguration, tenant: TenantName) -> Result<TenantEnvironment, TenantEnvironmentError> {
        let pool = connect_pool(configuration.database()).await?;
        let lease = SerializedLease::acquire(&pool).await?;
        Ok(TenantEnvironment {
            client: TestClient::new(configuration.broker().clone(), Some(tenant)), // clone: the environment owns its client endpoint while configuration remains reusable.
            lease,
            pool,
        })
    }

    /// Borrow the tenant-bound test client.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Release exclusive tenant-state ownership after caller-managed tenant removal.
    pub async fn finish(self) -> Result<(), TenantEnvironmentError> {
        let result = self.lease.release().await.map_err(TenantEnvironmentError::Isolation);
        self.pool.close().await;
        result
    }
}

/// Tenant environment allocation or release failure.
#[derive(Debug, Error)]
pub enum TenantEnvironmentError {
    /// Database connection or exclusive advisory-lock operation failed.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// Tenant header validation failed.
    #[error(transparent)]
    Tenant(#[from] TenantNameError),
}
