//! Private-tenant environment for tests that assert over a whole collection.
//!
//! A test that pages the entire `/subscriptions`, `/csourceRegistrations` or
//! `/csourceSubscriptions` collection and asserts the exact members cannot be
//! isolated by an `idPattern` -- those collections carry no such filter -- so it
//! runs against a tenant of its own, whose collections start empty. The broker
//! creates the tenant's database on the first write and names it
//! `andromeda_tenant_<name>_<suffix>`; finishing drops it so the databases do
//! not accumulate across a run.

use crate::{
    configuration::{ConfigurationError, TestRunConfiguration},
    database_isolation::{DatabaseIsolationError, connect_pool},
    identifiers::{IdentifierError, TestSuiteName},
    readiness::{ReadinessError, ReadinessPolicy, wait_until_ready_once},
    scope::Scope,
};
use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient, transport_error::TenantNameError};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

/// Test environment bound to a freshly named private tenant.
pub struct TenantScopedEnvironment {
    client: TestClient,
    scope: Scope,
    label: String,
    pool: PgPool,
}

impl TenantScopedEnvironment {
    /// Allocate a private tenant, a probe-free client bound to it, and an
    /// identifier scope, for a named suite.
    ///
    /// # Errors
    ///
    /// Returns [`TenantScopedEnvironmentError::Configuration`] when the process
    /// environment is invalid, [`TenantScopedEnvironmentError::Identifier`] when
    /// the suite name is not a valid token, [`TenantScopedEnvironmentError::Tenant`]
    /// when the generated tenant label is not a valid header value,
    /// [`TenantScopedEnvironmentError::Readiness`] when the broker does not answer
    /// its health endpoint, and [`TenantScopedEnvironmentError::Isolation`] when the
    /// pool cannot be opened.
    pub async fn from_suite(suite: &str) -> Result<TenantScopedEnvironment, TenantScopedEnvironmentError> {
        let configuration = TestRunConfiguration::from_environment()?;
        let suite = TestSuiteName::parse(suite.to_owned())?;
        let scope = Scope::allocate(&suite)?;
        // A database-safe, lowercase label so the broker's tenant database name
        // is `andromeda_tenant_<label>_<suffix>` and finish can find it by prefix.
        let label = format!("ts{}", &Uuid::new_v4().simple().to_string()[..12]);
        let tenant = TenantName::parse(&label)?;
        let client = TestClient::new(configuration.broker().clone(), Some(tenant)); // clone: the environment owns its client endpoint while configuration remains reusable.
        wait_until_ready_once(&client, ReadinessPolicy::default()).await?;
        let pool = connect_pool(configuration.database()).await?;
        Ok(TenantScopedEnvironment { client, scope, label, pool })
    }

    /// Borrow the tenant-bound HTTP client.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Borrow this environment's isolation scope for identifier allocation.
    pub const fn scope(&self) -> &Scope {
        &self.scope
    }

    /// Compose a wire identifier below this environment's isolated prefix.
    pub fn id(&self, local: &str) -> String {
        self.scope.id(local)
    }

    /// Borrow the non-empty cleanup prefix as wire text.
    pub fn id_prefix(&self) -> &str {
        self.scope.id_prefix()
    }

    /// Drop the private tenant's database and close the pool.
    ///
    /// The drop is best-effort: the tenant name is unique, so a database left
    /// behind never collides with another test, and it is reclaimed with the
    /// stack. A failure to reach the catalogue is therefore not a test failure.
    ///
    /// # Errors
    ///
    /// Never returns an error; the signature mirrors the other environments so a
    /// test can `.await.expect(...)` uniformly.
    pub async fn finish(self) -> Result<(), TenantScopedEnvironmentError> {
        drop_tenant_databases(&self.pool, &self.label).await;
        self.pool.close().await;
        Ok(())
    }
}

/// Force-drop every database backing the labelled tenant, ignoring failures.
async fn drop_tenant_databases(pool: &PgPool, label: &str) {
    let like = format!("andromeda_tenant_{label}_%");
    let databases: Vec<String> = match sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE $1")
        .bind(&like)
        .fetch_all(pool)
        .await
    {
        Ok(databases) => databases,
        Err(_error) => return,
    };
    for database in databases {
        // The name comes from `pg_database`, so it is a real identifier; `WITH
        // (FORCE)` evicts the tenant's background workers that hold it open.
        let statement = format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(pool).await;
    }
}

/// Private-tenant environment allocation failure.
#[derive(Debug, Error)]
pub enum TenantScopedEnvironmentError {
    /// Runtime test configuration is invalid.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Identifier allocation failed.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// The generated tenant label is not a valid header value.
    #[error(transparent)]
    Tenant(#[from] TenantNameError),
    /// The broker did not become ready within the configured policy.
    #[error(transparent)]
    Readiness(#[from] ReadinessError),
    /// Database connection could not be obtained.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
}
