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
use andromeda_test_client::{broker_endpoint::BrokerEndpoint, tenant_name::TenantName, test_client::TestClient, transport_error::TenantNameError};
use sqlx::PgPool;
use thiserror::Error;
use tokio::sync::OnceCell;
use uuid::Uuid;

/// Runs the leftover-tenant sweep exactly once per test process.
static TENANT_SWEEP: OnceCell<()> = OnceCell::const_new();

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
        sweep_stale_tenant_databases_once(&pool).await;
        Ok(TenantScopedEnvironment { client, scope, label, pool })
    }

    /// Borrow the tenant-bound HTTP client.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Borrow the private tenant's name, for suites that issue raw requests and
    /// must set the `NGSILD-Tenant` header themselves rather than go through
    /// [`TenantScopedEnvironment::client`].
    pub fn tenant(&self) -> &str {
        &self.label
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
        release_tenant(self.client.endpoint(), &self.label).await;
        drop_tenant_databases(&self.pool, &self.label).await;
        self.pool.close().await;
        Ok(())
    }
}

/// Ask the broker to suspend the tenant before its database is dropped, so the
/// database is never dropped out from under a cached tenant runtime.
///
/// The runtime cache keeps a pool per recently used tenant. Dropping a tenant's
/// database externally while the broker still holds its runtime leaves a stale
/// cache entry that makes the next tenant provisioned answer `503` until the
/// janitor evicts it -- which surfaces, under the base tier's cross-suite tenant
/// churn, as intermittent provisioning failures. Suspending through the
/// tenant-management extension (clause-4.14 lifecycle) closes the broker's pool
/// for this tenant first, so the following drop is clean. Best-effort: a broker
/// without the extension, or an unreachable endpoint, simply leaves the drop to
/// force the connections closed, so a failure here is never a test failure.
async fn release_tenant(endpoint: &BrokerEndpoint, label: &str) {
    let Ok(url) = endpoint.url().join(&format!("tenants/{label}")) else {
        return;
    };
    let _ = reqwest::Client::new()
        .patch(url)
        .header("content-type", "application/json")
        .body("{\"isActive\":false}")
        .send()
        .await;
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

/// Reclaim leftover tenant databases from earlier suites, once per test process.
///
/// Each base-tier suite is its own process and the runner executes them one at a
/// time, so when the first private tenant of a suite is allocated no other
/// suite's tenants are live. A private-tenant test only force-drops its own
/// database on `finish`, and a drop that races the broker's per-tenant
/// background worker can be silently skipped; across a whole tier those misses
/// pile up and each undropped database keeps a connection slot, eventually
/// starving new tenant provisioning (a 503). Sweeping every `andromeda_tenant_%`
/// database here, before this suite creates any of its own, bounds the
/// accumulation to a single suite's worth. It is guarded by [`TENANT_SWEEP`] so
/// the concurrent first allocations of a suite trigger it exactly once.
pub async fn sweep_stale_tenant_databases_once(pool: &PgPool) {
    TENANT_SWEEP.get_or_init(|| sweep_stale_tenant_databases(pool)).await;
}

/// Force-drop every `andromeda_tenant_%` database except the one in use, ignoring
/// failures.
async fn sweep_stale_tenant_databases(pool: &PgPool) {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(pool).await.unwrap_or_default();
    let databases: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' AND datname <> $1")
        .bind(&current)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for database in databases {
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
