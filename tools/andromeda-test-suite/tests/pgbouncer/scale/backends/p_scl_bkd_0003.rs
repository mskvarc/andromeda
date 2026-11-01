//! A broker without a pooler keeps several times the backends once a burst ends.

use andromeda_test_suite::{
    backend_sampler::{BackendSampler, BackendSeries},
    broker,
    config::{BrokerRole, SuiteConfig},
    load_run::run_load,
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::sleep;

/// The tenants of each burst: inside the 32 each broker keeps resident.
const TENANTS: usize = 24;

/// The workers of each burst.
const WORKERS: usize = 32;

/// How long after a burst its held backends are counted: past the 5 s the pooler keeps a server after its last
/// transaction, inside the time a broker's own pool keeps an idle connection.
const SETTLE: Duration = Duration::from_secs(12);

/// The window the held backends are counted over.
const WINDOW: Duration = Duration::from_secs(5);

/// The client backends on tenant databases the broker of `role` holds once a burst of a Retrieve Entity or, one in
/// four, an Update Attributes over 24 tenants ended and settled.
async fn held_after_burst(role: BrokerRole, load: Duration) -> Result<BackendSeries> {
    let client = broker::quiet_client(role)?;
    let set = TenantSet::provision(&client, &Namespace::fresh(), TENANTS, TENANTS).await?;
    let picker = UniformPicker::new(set.len(), 7)?;
    let burst = run_load(WORKERS, load, |number| {
        let tenant = picker.pick();
        let set = &set;
        async move { if number % 4 == 0 { set.update(tenant).await } else { set.read(tenant).await } }
    })
    .await;
    println!("{role:?}: {burst}");
    sleep(SETTLE).await;
    Ok(BackendSampler::over(ServerBackends::of(role)?, BackendScope::TenantDatabases, WINDOW).await?)
}

/// Pins: after the same burst of 32 workers over 24 tenants for the scale tests' load duration, and 12 s to settle,
/// a broker reaching its server directly still holds at least one client backend per tenant, and at least three
/// times as many as a broker behind a transaction pooler that releases a server 5 s after its last transaction
/// Clause: 5.7.1
/// Tenant: private (the subject is the backends of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: one broker serves its requests through a transaction-pooling
/// `PgBouncer`, the other holds four connections per tenant of its own
#[gtest]
#[tokio::test]
async fn p_scl_bkd_0003_a_broker_without_a_pooler_keeps_several_times_the_backends_once_a_burst_ends() -> Result<()> {
    let load = SuiteConfig::get()?.pooling()?.load_duration();
    let (pooled, direct) = tokio::join!(held_after_burst(BrokerRole::BurstPooled, load), held_after_burst(BrokerRole::BurstDirect, load));
    let (pooled, direct) = (pooled?, direct?);
    println!("held: pooled {pooled:?}, direct {direct:?}");
    expect_that!(direct.trough, ge(u64::try_from(TENANTS)?));
    verify_that!(pooled.trough * 3, le(direct.trough))
}
