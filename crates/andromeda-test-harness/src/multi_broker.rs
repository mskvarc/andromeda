//! Role-neutral lifecycle for real multi-broker integration topologies.

use crate::{
    configuration::{ConfigurationError, DatabaseEndpoint},
    database_isolation::{DatabaseIsolationError, connect_pool},
    database_reset::{DatabaseResetError, cleanup_prefix, reset_owned_tenant},
    identifiers::{IdentifierError, TestIdentifiers, TestSuiteName},
    readiness::{ReadinessError, ReadinessPolicy, wait_until_ready},
};
use andromeda_test_client::{
    broker_endpoint::{BrokerDnsOverride, BrokerEndpoint},
    tenant_name::TenantName,
    test_client::TestClient,
};
use http::HeaderValue;
use sqlx::PgPool;
use std::{env, fmt::Debug, net::SocketAddr, slice};
use thiserror::Error;
use url::Url;

/// The environment-variable name fragments that select one broker's overrides.
///
/// Every setting is read from `ANDROMEDA_{prefix}_{token}_{suffix}`: the prefix
/// scopes one composed topology, the token names a single broker inside it, and
/// the suffix is fixed per setting.
#[derive(Clone, Copy, Debug)]
pub struct BrokerEnvironmentKey<'a> {
    prefix: &'a str,
    token: &'a str,
}

impl<'a> BrokerEnvironmentKey<'a> {
    /// Bind a topology-wide prefix to the token naming one broker within it.
    #[must_use]
    pub const fn new(prefix: &'a str, token: &'a str) -> BrokerEnvironmentKey<'a> {
        BrokerEnvironmentKey { prefix, token }
    }

    /// Read one suffixed setting, falling back to its compiled-in default when the variable is unset or empty.
    fn value(self, suffix: &str, default: &str) -> String {
        let name = format!("ANDROMEDA_{}_{}_{suffix}", self.prefix, self.token);
        match env::var(name) {
            Ok(value) if !value.is_empty() => value,
            Ok(_) | Err(_) => default.to_owned(),
        }
    }
}

/// The compiled-in endpoints one broker falls back to when nothing overrides them.
///
/// Each field is the default for the matching variable named by a
/// [`BrokerEnvironmentKey`], in the order the specification reads them.
#[derive(Clone, Copy, Debug)]
pub struct BrokerNodeDefaults<'a> {
    client: &'a str,
    peer: &'a str,
    database: &'a str,
    alias: &'a str,
}

impl<'a> BrokerNodeDefaults<'a> {
    /// Collect the host-side client URL, the compose-network peer URL, the database URL, and the source alias.
    #[must_use]
    pub const fn new(client: &'a str, peer: &'a str, database: &'a str, alias: &'a str) -> BrokerNodeDefaults<'a> {
        BrokerNodeDefaults { client, peer, database, alias }
    }
}

/// Declarative configuration for one broker role in a composed topology.
#[derive(Clone)]
pub struct BrokerNodeSpec<R> {
    role: R,
    identity: R,
    client: BrokerEndpoint,
    peer: Url,
    database: DatabaseEndpoint,
    alias: String,
    dns_override: Option<BrokerDnsOverride>,
}

impl<R> BrokerNodeSpec<R>
where
    R: Copy + Debug,
{
    /// Build a broker specification from environment overrides and typed defaults.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Configuration`] when the resolved client URL or database URL is invalid, and [`MultiBrokerError::Endpoint`] when the resolved peer URL does not parse.
    pub fn from_environment(
        role: R,
        identity: R,
        key: BrokerEnvironmentKey<'_>,
        defaults: BrokerNodeDefaults<'_>,
        dns_port: Option<u16>,
    ) -> Result<BrokerNodeSpec<R>, MultiBrokerError> {
        let client_value = key.value("URL", defaults.client);
        let peer_value = key.value("PEER_URL", defaults.peer);
        let database_value = key.value("DATABASE_URL", defaults.database);
        let alias = key.value("ALIAS", defaults.alias);
        let client = BrokerEndpoint::parse(client_value).map_err(|error| MultiBrokerError::Configuration(ConfigurationError::InvalidBrokerEndpoint(error)))?;
        let peer = Url::parse(&peer_value).map_err(|error| MultiBrokerError::Endpoint(error.to_string()))?;
        let database = DatabaseEndpoint::parse(database_value).map_err(MultiBrokerError::Configuration)?;
        let dns_override = dns_port.map(|port| {
            let host = peer.host_str().unwrap_or_default().to_owned();
            BrokerDnsOverride::new(host, SocketAddr::from(([127, 0, 0, 1], port)))
        });
        Ok(BrokerNodeSpec {
            role,
            identity,
            client,
            peer,
            database,
            alias,
            dns_override,
        })
    }

    /// Return this role's value.
    pub const fn role(&self) -> R {
        self.role
    }

    /// Return the identity whose store and source alias this role uses.
    pub const fn identity(&self) -> R {
        self.identity
    }

    /// Borrow the host-side broker endpoint.
    pub const fn client(&self) -> &BrokerEndpoint {
        &self.client
    }

    /// Borrow the compose-network endpoint used in registrations.
    pub const fn peer(&self) -> &Url {
        &self.peer
    }

    /// Borrow this broker's database endpoint.
    pub const fn database(&self) -> &DatabaseEndpoint {
        &self.database
    }

    /// Borrow this broker's source alias.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Borrow the optional host-side DNS override.
    pub const fn dns_override(&self) -> Option<&BrokerDnsOverride> {
        self.dns_override.as_ref()
    }
}

/// A live broker handle owned by a [`MultiBrokerTopology`].
pub struct BrokerHandle<R> {
    role: R,
    identity: R,
    client: TestClient,
    endpoint: BrokerEndpoint,
    peer: Url,
    alias: String,
    pool: PgPool,
    context: Option<HeaderValue>,
}

impl<R> BrokerHandle<R>
where
    R: Copy,
{
    /// Borrow the broker's HTTP client.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Return this broker's role.
    pub const fn role(&self) -> R {
        self.role
    }

    /// Return the role whose durable store this broker shares.
    pub const fn identity(&self) -> R {
        self.identity
    }

    /// Borrow the endpoint to place in a Context Source Registration.
    pub fn peer_url(&self) -> &str {
        self.peer.as_str()
    }

    /// Borrow the broker's source alias.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Borrow the broker's database pool for isolation assertions.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Build a client for this broker under another tenant.
    pub fn tenant_client(&self, tenant: TenantName) -> TestClient {
        let client = TestClient::new(self.endpoint.clone(), Some(tenant));
        match self.context.clone() {
            Some(link) => client.with_default_context(link),
            None => client,
        }
    }
}

/// A live, prefix-isolated topology of independently stored brokers.
pub struct MultiBrokerTopology<R> {
    identifiers: TestIdentifiers,
    brokers: Vec<BrokerHandle<R>>,
}

impl<R> MultiBrokerTopology<R>
where
    R: Copy + Eq + Debug + Send + Sync,
{
    /// Build all requested brokers, wait for readiness, and reset each unique store.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Identifier`] when the suite name or the allocated prefix is invalid, [`MultiBrokerError::Client`] when a DNS override cannot be applied, [`MultiBrokerError::Readiness`] when a broker never becomes ready, [`MultiBrokerError::Database`] when a broker pool cannot be opened, and [`MultiBrokerError::Reset`] when resetting an owned store fails.
    pub async fn from_specs(
        suite: &str,
        specs: Vec<BrokerNodeSpec<R>>,
        context: Option<HeaderValue>,
        readiness: ReadinessPolicy,
    ) -> Result<MultiBrokerTopology<R>, MultiBrokerError> {
        let suite = TestSuiteName::parse(suite)?;
        let identifiers = TestIdentifiers::allocate(&suite)?;
        let mut brokers = Vec::with_capacity(specs.len());
        for spec in specs {
            let mut client = TestClient::new(spec.client.clone(), None);
            if let Some(dns_override) = spec.dns_override.as_ref() {
                client = client
                    .with_dns_overrides(slice::from_ref(dns_override))
                    .map_err(|error| MultiBrokerError::Client(error.to_string()))?;
            }
            if let Some(link) = context.as_ref() {
                client = client.with_default_context(link.clone());
            }
            wait_until_ready(&client, readiness).await.map_err(|error| MultiBrokerError::Readiness {
                role: format!("{:?}", spec.role),
                source: error,
            })?;
            let pool = connect_pool(&spec.database).await?;
            if !brokers.iter().any(|broker: &BrokerHandle<R>| broker.role == spec.identity) {
                reset_owned_tenant(&pool).await?;
            }
            brokers.push(BrokerHandle {
                role: spec.role,
                identity: spec.identity,
                client,
                endpoint: spec.client,
                peer: spec.peer,
                alias: spec.alias,
                pool,
                context: context.clone(),
            });
        }
        Ok(MultiBrokerTopology { identifiers, brokers })
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

    /// Clean every unique store and close all pools, preserving all cleanup errors.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Cleanup`] carrying every per-broker prefix-cleanup failure encountered while draining the topology.
    pub async fn finish(self) -> Result<(), MultiBrokerError> {
        let mut failures = Vec::new();
        let mut cleaned = Vec::new();
        for broker in &self.brokers {
            if cleaned.contains(&broker.identity) {
                continue;
            }
            if let Err(error) = cleanup_prefix(broker.pool(), &self.identifiers).await {
                failures.push(format!("broker {:?}: {error}", broker.role));
            }
            cleaned.push(broker.identity);
        }
        for broker in self.brokers {
            broker.pool.close().await;
        }
        if failures.is_empty() {
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
    /// The requested role was not materialised.
    #[error("topology does not contain broker role {0}")]
    MissingRole(String),
    /// One or more stores could not be cleaned.
    #[error("topology cleanup failed: {0}")]
    Cleanup(String),
}
