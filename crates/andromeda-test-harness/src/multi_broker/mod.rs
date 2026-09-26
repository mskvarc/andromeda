//! Role-neutral lifecycle for real multi-broker integration topologies.
//!
//! Each topology runs under a tenant leased exclusively to it
//! ([`crate::tenant_lease`]) and scopes its resources by a unique identifier
//! prefix inside that tenant.

pub mod broker_handle;
pub mod node_spec;

use crate::{
    configuration::ConfigurationError,
    database_isolation::{DatabaseIsolationError, connect_pool},
    database_reset::{DatabaseResetError, cleanup_prefix, reset_owned_tenant},
    identifiers::{IdentifierError, TestIdentifiers, TestSuiteName},
    multi_broker::{broker_handle::BrokerHandle, node_spec::BrokerNodeSpec},
    readiness::{ReadinessError, ReadinessPolicy, wait_until_ready},
    tenant_lease::TenantLease,
    tenant_provisioning::{TenantProvisioningError, provisioned_tenant_database},
};
use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient, transport_error::TenantNameError};
use dashmap::DashMap;
use http::HeaderValue;
use sqlx::PgPool;
use std::{
    fmt::Debug,
    slice,
    sync::{Arc, LazyLock},
};
use thiserror::Error;
use tokio::sync::OnceCell;
use url::Url;

/// Per-process registry ensuring each broker's platform database is reset
/// exactly once, however many topologies a suite constructs.
///
/// `distop` and `fed` build a fresh topology for every test, but the reset only
/// has to clear what a previous run of the binary left behind: within one
/// process each test runs under its own leased tenant, scopes its resources by a
/// unique id prefix and reclaims them in [`MultiBrokerTopology::finish`].
/// Resetting once per database rather than once per test is what lets the
/// suites run concurrently -- a mid-test `TRUNCATE` from a second topology would
/// otherwise wipe the first test's data.
static RESET_DATABASES: LazyLock<DashMap<Url, Arc<OnceCell<()>>>> = LazyLock::new(DashMap::new);

/// Reset one broker database the first time this process names it, and only then.
///
/// Concurrent constructors that name the same database all await the single
/// in-flight reset; every later constructor observes the initialised cell and
/// skips the reset. A failed reset leaves the cell uninitialised, so the next
/// constructor retries it.
async fn reset_database_once(pool: &PgPool, database: &Url) -> Result<(), MultiBrokerError> {
    let cell = Arc::clone(RESET_DATABASES.entry(database.clone()).or_default().value()); // clone: the registry key owns the database it names
    cell.get_or_try_init(|| reset_owned_tenant(pool)).await?;
    Ok(())
}

/// Build a client for one broker of the topology under `tenant`, or under the
/// default tenant when `tenant` is `None`.
fn broker_client<R>(spec: &BrokerNodeSpec<R>, tenant: Option<TenantName>, context: Option<&HeaderValue>) -> Result<TestClient, MultiBrokerError>
where
    R: Copy + Debug,
{
    let mut client = TestClient::new(spec.client().clone(), tenant);
    if let Some(dns_override) = spec.dns_override() {
        client = client
            .with_dns_overrides(slice::from_ref(dns_override))
            .map_err(|error| MultiBrokerError::Client(error.to_string()))?;
    }
    Ok(match context {
        Some(link) => client.with_default_context(link.clone()), // clone: every client owns its default context header
        None => client,
    })
}

/// A live, tenant-leased and prefix-isolated topology of independently stored
/// brokers.
pub struct MultiBrokerTopology<R> {
    identifiers: TestIdentifiers,
    lease: TenantLease,
    brokers: Vec<BrokerHandle<R>>,
}

impl<R> MultiBrokerTopology<R>
where
    R: Copy + Eq + Debug + Send + Sync,
{
    /// Lease a tenant, build all requested brokers under it, wait for
    /// readiness, and reset each store the first time this process connects to
    /// it (see [`RESET_DATABASES`] and [`crate::tenant_provisioning`]).
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Identifier`] when the suite name or the allocated prefix is invalid, [`MultiBrokerError::Tenant`] when the leased tenant is not a valid header value, [`MultiBrokerError::Client`] when a DNS override cannot be applied, [`MultiBrokerError::Readiness`] when a broker never becomes ready, [`MultiBrokerError::Database`] when a broker pool cannot be opened, [`MultiBrokerError::Reset`] when resetting an owned store fails, and [`MultiBrokerError::Provisioning`] when the leased tenant cannot be created on a broker.
    pub async fn from_specs(
        suite: &str,
        specs: Vec<BrokerNodeSpec<R>>,
        context: Option<HeaderValue>,
        readiness: ReadinessPolicy,
    ) -> Result<MultiBrokerTopology<R>, MultiBrokerError> {
        let suite = TestSuiteName::parse(suite)?;
        let identifiers = TestIdentifiers::allocate(&suite)?;
        let lease = TenantLease::acquire(&suite)?;
        let mut brokers = Vec::with_capacity(specs.len());
        for spec in specs {
            let default_client = broker_client(&spec, None, context.as_ref())?;
            wait_until_ready(&default_client, readiness)
                .await
                .map_err(|error| MultiBrokerError::Readiness {
                    role: format!("{:?}", spec.role()),
                    source: error,
                })?;
            let platform_pool = connect_pool(spec.database()).await?;
            reset_database_once(&platform_pool, spec.database().url()).await?;
            let client = broker_client(&spec, Some(lease.tenant().clone()), context.as_ref())?; // clone: every client owns the tenant it runs under
            let tenant_database = provisioned_tenant_database(&client, &platform_pool, spec.database(), lease.slot()).await?;
            let tenant_pool = connect_pool(&tenant_database).await?;
            brokers.push(BrokerHandle {
                role: spec.role(),
                identity: spec.identity(),
                client,
                default_client,
                endpoint: spec.client().clone(), // clone: the handle builds further tenant clients from the endpoint
                peer: spec.peer().clone(),       // clone: the handle owns the endpoint it names in registrations
                alias: spec.alias().to_owned(),
                tenant_pool,
                platform_pool,
                context: context.clone(), // clone: every broker handle owns the default context of its clients
            });
        }
        Ok(MultiBrokerTopology { identifiers, lease, brokers })
    }

    /// Borrow a broker by its typed role.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::MissingRole`] when the topology was not built with a broker for `role`.
    pub fn broker(&self, role: R) -> Result<&BrokerHandle<R>, MultiBrokerError> {
        self.brokers
            .iter()
            .find(|broker| broker.role == role)
            .ok_or_else(|| MultiBrokerError::MissingRole(format!("{role:?}")))
    }

    /// Compose an isolated identifier for any participating broker.
    #[must_use]
    pub fn id(&self, local: &str) -> String {
        format!("{}:{local}", self.identifiers.prefix())
    }

    /// Borrow the prefix used by this topology's cleanup.
    #[must_use]
    pub fn id_prefix(&self) -> &str {
        self.identifiers.prefix().as_str()
    }

    /// Clean every unique store under the leased tenant and the default tenant,
    /// close all pools, and hand the tenant back, preserving all cleanup errors.
    ///
    /// A topology whose cleanup failed abandons its tenant rather than handing
    /// back one that still holds its resources.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Cleanup`] carrying every per-broker prefix-cleanup failure encountered while draining the topology.
    pub async fn finish(self) -> Result<(), MultiBrokerError> {
        let MultiBrokerTopology { identifiers, lease, brokers } = self;
        let mut failures = Vec::new();
        let mut cleaned = Vec::new();
        for broker in &brokers {
            if cleaned.contains(&broker.identity) {
                continue;
            }
            for pool in [&broker.tenant_pool, &broker.platform_pool] {
                if let Err(error) = cleanup_prefix(pool, &identifiers).await {
                    failures.push(format!("broker {:?}: {error}", broker.role));
                }
            }
            cleaned.push(broker.identity);
        }
        for broker in brokers {
            broker.tenant_pool.close().await;
            broker.platform_pool.close().await;
        }
        if failures.is_empty() {
            lease.release();
            Ok(())
        } else {
            Err(MultiBrokerError::Cleanup(failures.join("; ")))
        }
    }
}

/// Failure while constructing, starting, or cleaning a multi-broker topology.
#[derive(Debug, Error)]
pub enum MultiBrokerError {
    /// Identifier allocation failed.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// Environment or endpoint configuration failed.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// A peer endpoint was not a valid URL.
    #[error("invalid peer endpoint: {0}")]
    Endpoint(String),
    /// The leased tenant is not a valid header value.
    #[error(transparent)]
    Tenant(#[from] TenantNameError),
    /// The test client could not be built.
    #[error("test client setup failed: {0}")]
    Client(String),
    /// A broker did not become ready.
    #[error("broker {role} did not become ready: {source}")]
    Readiness {
        /// Role that failed readiness.
        role: String,
        /// Readiness failure.
        source: ReadinessError,
    },
    /// A broker database could not be connected.
    #[error(transparent)]
    Database(#[from] DatabaseIsolationError),
    /// Resetting a broker store failed.
    #[error(transparent)]
    Reset(#[from] DatabaseResetError),
    /// The leased tenant could not be created on a broker.
    #[error(transparent)]
    Provisioning(#[from] TenantProvisioningError),
    /// The requested role was not materialised.
    #[error("topology does not contain broker role {0}")]
    MissingRole(String),
    /// One or more stores could not be cleaned.
    #[error("topology cleanup failed: {0}")]
    Cleanup(String),
}
