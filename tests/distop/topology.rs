//! Multi-broker topology fixture for the distributed-operations conformance suite.
//!
//! Reads the `distributed.env` variables documented in
//! `docker/distributed/distributed.env.example`, builds a [`TestClient`] against
//! each running broker, waits for every required instance to be healthy, and on
//! [`DistributedTopology::finish`] runs prefix-scoped cleanup against every
//! instance's own database.
//!
//! Two URL families per broker are kept strictly apart (clause 4.3.6 / 6.3.15):
//! a *client URL* the host-side test uses to reach a broker, and a *peer URL*
//! that another broker uses over the compose network and that therefore is the
//! only value allowed inside a Context Source Registration `endpoint`. Only
//! [`BrokerHandle::peer_url`] exposes the latter, so a test cannot accidentally
//! register a `localhost` endpoint a peer cannot reach.

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, header::jsonld_link_header, tenant_name::TenantName, test_client::TestClient};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::{
    configuration::DatabaseEndpoint,
    database_isolation::connect_pool,
    database_reset::{cleanup_prefix, reset_owned_tenant},
    identifiers::{IdentifierError, TestIdentifiers, TestSuiteName},
    readiness::{ReadinessPolicy, wait_until_ready},
};
use sqlx::PgPool;
use std::{env, time::Duration};
use thiserror::Error;

/// The suite name embedded in every isolated identifier prefix.
const SUITE: &str = "distop";

/// Health-poll budget for a cold compose broker: generous enough for a fresh
/// container that still has to migrate its schema before answering `/health`.
const READINESS: ReadinessPolicy = ReadinessPolicy::new(Duration::from_secs(45), Duration::from_millis(250), Duration::from_secs(2));

/// One broker instance in the compose topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerRole {
    /// The front / distribution broker under test (host port 1061).
    A,
    /// A second instance of the broker-a identity (host port 1064): same
    /// source id, alias and database as [`BrokerRole::A`]. Durable state
    /// minted through A is visible here and on no other role.
    AReplica,
    /// The first peer / context source (host port 1062).
    B,
    /// The second peer, for redirect fan-out and A->B->C chains (host port 1063).
    C,
}

impl BrokerRole {
    /// The token used in this role's URL environment keys.
    const fn token(self) -> &'static str {
        match self {
            BrokerRole::A => "A",
            BrokerRole::AReplica => "A_REPLICA",
            BrokerRole::B => "B",
            BrokerRole::C => "C",
        }
    }

    /// The broker identity this role serves: the role whose alias and store
    /// it shares. A replica resolves to its primary; every other role to itself.
    const fn identity(self) -> BrokerRole {
        match self {
            BrokerRole::A | BrokerRole::AReplica => BrokerRole::A,
            BrokerRole::B => BrokerRole::B,
            BrokerRole::C => BrokerRole::C,
        }
    }

    /// The per-identity database URL default when the environment does not set one.
    const fn default_database_url(self) -> &'static str {
        match self.identity() {
            BrokerRole::A | BrokerRole::AReplica => "postgres://postgres:postgres@127.0.0.1:5442/broker_a",
            BrokerRole::B => "postgres://postgres:postgres@127.0.0.1:5443/broker_b",
            BrokerRole::C => "postgres://postgres:postgres@127.0.0.1:5444/broker_c",
        }
    }

    /// The host-facing client URL default when the environment does not set one.
    const fn default_client_url(self) -> &'static str {
        match self {
            BrokerRole::A => "http://127.0.0.1:1061/ngsi-ld/v1/",
            BrokerRole::AReplica => "http://127.0.0.1:1064/ngsi-ld/v1/",
            BrokerRole::B => "http://127.0.0.1:1062/ngsi-ld/v1/",
            BrokerRole::C => "http://127.0.0.1:1063/ngsi-ld/v1/",
        }
    }

    /// The compose-network peer URL default when the environment does not set
    /// one. This is the broker origin only: a Context Source Registration
    /// `endpoint` is the base a broker appends the full `/ngsi-ld/v1/...` path
    /// to, so including the API path here would double it on every forward.
    const fn default_peer_url(self) -> &'static str {
        match self {
            BrokerRole::A => "http://broker-a:1056",
            BrokerRole::AReplica => "http://broker-a2:1056",
            BrokerRole::B => "http://broker-b:1056",
            BrokerRole::C => "http://broker-c:1056",
        }
    }

    /// This broker's Context Source alias default when the environment does not set one.
    const fn default_alias(self) -> &'static str {
        match self.identity() {
            BrokerRole::A | BrokerRole::AReplica => "broker-a",
            BrokerRole::B => "broker-b",
            BrokerRole::C => "broker-c",
        }
    }
}

/// A single broker: its client, the peer URL other brokers register, its alias,
/// and the database pool used to reclaim prefix-owned rows on teardown.
pub struct BrokerHandle {
    role: BrokerRole,
    client: TestClient,
    endpoint: BrokerEndpoint,
    peer_url: String,
    alias: String,
    pool: PgPool,
}

impl BrokerHandle {
    /// The host-side client bound to this broker (default tenant).
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// The peer URL to embed inside a Context Source Registration `endpoint` so
    /// another broker reaches this one over the compose network.
    pub fn peer_url(&self) -> &str {
        &self.peer_url
    }

    /// This broker's configured Context Source alias (its federation identity).
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// The database pool for this broker, for persistence assertions.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Build an additional client for this broker bound to a specific tenant.
    pub fn tenant_client(&self, tenant: TenantName) -> TestClient {
        // clone: each tenant view owns its endpoint and default context while the broker keeps its default-tenant client.
        let client = TestClient::new(self.endpoint.clone(), Some(tenant));
        match self.client.default_context() {
            Some(link) => client.with_default_context(link.clone()),
            None => client,
        }
    }
}

/// A materialised multi-broker topology with a shared isolated identifier prefix.
pub struct DistributedTopology {
    identifiers: TestIdentifiers,
    brokers: Vec<BrokerHandle>,
}

impl DistributedTopology {
    /// Materialise a single-broker topology (broker A only): baseline `T-LOCAL`
    /// and every `T-MOCK` case, where the peer is an in-process mock the test
    /// starts and registers itself.
    pub async fn single() -> Result<DistributedTopology, TopologyError> {
        DistributedTopology::with_roles(&[BrokerRole::A]).await
    }

    /// Materialise a two-broker topology (A front, B real peer): `T-PAIR`,
    /// `T-LOOP`, and `T-TENANT`.
    pub async fn pair() -> Result<DistributedTopology, TopologyError> {
        DistributedTopology::with_roles(&[BrokerRole::A, BrokerRole::B]).await
    }

    /// Materialise a three-broker topology (A -> B -> C): `T-CHAIN` and
    /// `T-SPLIT`, where B and C each hold part of the same entity.
    pub async fn chain() -> Result<DistributedTopology, TopologyError> {
        DistributedTopology::with_roles(&[BrokerRole::A, BrokerRole::B, BrokerRole::C]).await
    }

    /// Materialise the broker-a replica pair plus one peer (A, A2, B): state
    /// minted through A must be visible on A2 and absent on B.
    pub async fn replica_pair() -> Result<DistributedTopology, TopologyError> {
        DistributedTopology::with_roles(&[BrokerRole::A, BrokerRole::AReplica, BrokerRole::B]).await
    }

    async fn with_roles(roles: &[BrokerRole]) -> Result<DistributedTopology, TopologyError> {
        let suite = TestSuiteName::parse(SUITE)?;
        let identifiers = TestIdentifiers::allocate(&suite)?;
        let mut brokers: Vec<BrokerHandle> = Vec::with_capacity(roles.len());
        for &role in roles {
            let broker = build_broker(role).await?;
            // Reset each broker's default tenant to empty before the test runs.
            // A broker's registration namespace is global, so state a previous
            // test leaked - especially when it panicked before its own cleanup -
            // would poison this one (a stale exclusive registration alone turns
            // every local create into a 409). Resetting on construction makes
            // each test start from a clean broker; it requires the suite to run
            // single-threaded, which it already does. A replica shares its
            // primary's store, which was reset when the primary was built.
            if !brokers.iter().any(|built| built.role.identity() == role.identity()) {
                reset_owned_tenant(broker.pool())
                    .await
                    .map_err(|error| TopologyError::Database(error.to_string()))?;
            }
            brokers.push(broker);
        }
        Ok(DistributedTopology { identifiers, brokers })
    }

    /// Borrow the handle for one broker role. Panics if the role is not part of
    /// this topology, which is a test-authoring error, not a runtime condition.
    pub fn broker(&self, role: BrokerRole) -> &BrokerHandle {
        self.brokers
            .iter()
            .find(|broker| broker.role == role)
            .unwrap_or_else(|| unreachable!("topology does not include broker {role:?}"))
    }

    /// Shorthand for the front broker under test (role A).
    pub fn front(&self) -> &BrokerHandle {
        self.broker(BrokerRole::A)
    }

    /// Compose a wire identifier below this run's isolated cleanup prefix, shared
    /// across every broker so cleanup reclaims forwarded copies too.
    pub fn id(&self, local: &str) -> String {
        format!("{}:{local}", self.identifiers.prefix())
    }

    /// Borrow the isolated cleanup prefix as wire text.
    pub fn id_prefix(&self) -> &str {
        self.identifiers.prefix().as_str()
    }

    /// Reclaim every prefix-owned row on each broker's database, then close the
    /// pools. Failures on individual brokers are aggregated so one broker's
    /// cleanup error never hides another's.
    pub async fn finish(self) -> Result<(), TopologyError> {
        let mut failures = Vec::new();
        let mut cleaned: Vec<BrokerRole> = Vec::with_capacity(self.brokers.len());
        for broker in &self.brokers {
            // One store per identity: cleaning the primary already reclaimed
            // every row its replica could see.
            if cleaned.contains(&broker.role.identity()) {
                continue;
            }
            if let Err(error) = cleanup_prefix(broker.pool(), &self.identifiers).await {
                failures.push(format!("broker {:?}: {error}", broker.role));
            }
            cleaned.push(broker.role.identity());
        }
        for broker in self.brokers {
            broker.pool.close().await;
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(TopologyError::Cleanup(failures.join("; ")))
        }
    }
}

/// Read a broker's configuration from the environment and connect to it.
async fn build_broker(role: BrokerRole) -> Result<BrokerHandle, TopologyError> {
    let client_url = env_or(&format!("ANDROMEDA_DIST_{}_URL", role.token()), role.default_client_url());
    let peer_url = env_or(&format!("ANDROMEDA_DIST_{}_PEER_URL", role.token()), role.default_peer_url());
    let alias = env_or(&format!("ANDROMEDA_DIST_{}_ALIAS", role.identity().token()), role.default_alias());
    let endpoint = BrokerEndpoint::parse(&client_url).map_err(|error| TopologyError::Endpoint(error.to_string()))?;
    // Every fixture of this suite is expressed in the compound test-suite
    // context; requests that name no context of their own use it as well, so
    // the terms a test registers, creates and queries denote the same IRIs.
    let context_link = jsonld_link_header(COMPOUND_CONTEXT.as_str()).map_err(|error| TopologyError::Endpoint(error.to_string()))?;
    let client = TestClient::new(endpoint.clone(), None).with_default_context(context_link);
    wait_until_ready(&client, READINESS).await.map_err(|error| TopologyError::Readiness {
        role: format!("{role:?}"),
        detail: error.to_string(),
    })?;
    let database = broker_database(role)?;
    // One pool per broker per topology, never a process-global pool: sqlx binds
    // a pool's connections and background tasks to the reactor that created
    // them, and this suite runs single-threaded with a fresh current-thread
    // runtime per `#[tokio::test]`. A shared `OnceLock<PgPool>` would strand
    // dead connections once the first test's runtime is dropped. Per-topology
    // pools stay within `max_connections` via the 2-connection cap and 5s
    // acquire timeout in `database_isolation::connect_pool`.
    let pool = connect_pool(&database).await.map_err(|error| TopologyError::Database(error.to_string()))?;
    Ok(BrokerHandle {
        role,
        client,
        endpoint,
        peer_url,
        alias,
        pool,
    })
}

/// Resolve a broker's database endpoint: `ANDROMEDA_DIST_<R>_DATABASE_URL` for
/// the role's identity (a replica reads its primary's variable), falling back
/// to the compose stack's per-identity server and database.
fn broker_database(role: BrokerRole) -> Result<DatabaseEndpoint, TopologyError> {
    let url = env_or(&format!("ANDROMEDA_DIST_{}_DATABASE_URL", role.identity().token()), role.default_database_url());
    DatabaseEndpoint::parse(url).map_err(|error| TopologyError::Database(error.to_string()))
}

/// Read an environment variable, falling back to a default when unset or empty.
fn env_or(name: &str, default: &str) -> String {
    match env::var(name) {
        Ok(value) if !value.is_empty() => value,
        Ok(_) | Err(_) => default.to_owned(),
    }
}

/// Topology setup or teardown failure.
#[derive(Debug, Error)]
pub enum TopologyError {
    /// A configured identifier prefix could not be allocated.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// A broker client URL was invalid.
    #[error("invalid broker client URL: {0}")]
    Endpoint(String),
    /// A broker never became healthy within the readiness budget.
    #[error("broker {role} did not become ready: {detail}")]
    Readiness {
        /// The broker role that failed to become ready.
        role: String,
        /// The underlying readiness failure.
        detail: String,
    },
    /// A broker database could not be resolved or connected.
    #[error("broker database error: {0}")]
    Database(String),
    /// Prefix cleanup failed on one or more brokers.
    #[error("topology cleanup failed: {0}")]
    Cleanup(String),
}
