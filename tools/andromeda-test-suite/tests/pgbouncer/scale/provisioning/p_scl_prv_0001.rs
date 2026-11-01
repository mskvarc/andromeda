//! A burst of first writes stays within the server's connections.

use andromeda_test_suite::{
    backend_sampler::BackendSampler,
    broker,
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_set::TenantSet,
};
use googletest::prelude::*;

/// The first writes sent at once.
const CONCURRENCY: usize = 32;

/// The connections the broker's server admits (`max_connections`): a broker that reached them would have its next
/// connection refused.
const SERVER_CONNECTIONS: u64 = 300;

/// Pins: the first writes of the scale tests' tenant count, 32 at a time, through a broker behind a transaction
/// pooler, each create their tenant (201), and the client backends on the broker's server stay below its 300
/// connections throughout
/// Clause: 5.5.10, 5.6.1
/// Tenant: private (the subject is the implicit creation of many tenants by their first write)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and creates and migrates each tenant database over a direct connection
#[gtest]
#[tokio::test]
async fn p_scl_prv_0001_a_burst_of_first_writes_stays_within_the_servers_connections() -> Result<()> {
    let tenants = SuiteConfig::get()?.pooling()?.scale_tenants();
    let provisioning = broker::quiet_client(BrokerRole::Provisioning)?;
    let sampler = BackendSampler::start(ServerBackends::of(BrokerRole::Provisioning)?, BackendScope::WholeServer);
    let provisioned = TenantSet::provision(&provisioning, &Namespace::fresh(), tenants, CONCURRENCY).await;
    let series = sampler.stop().await?;
    println!("{series:?}");
    expect_that!(provisioned.map(|set| set.len()).ok(), some(eq(tenants)));
    verify_that!(series.peak, lt(SERVER_CONNECTIONS))
}
