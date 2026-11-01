//! An idle cache of every tenant releases its backends.

use andromeda_test_suite::{
    backend_sampler::BackendSampler,
    broker,
    broker_metrics::{BrokerMetric, MetricsReading},
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::sleep;

/// The first writes sent at once while the tenants are brought up.
const CONCURRENCY: usize = 32;

/// How long the tenants stay idle before their backends are counted: well past the 5 s the pooler keeps a server after
/// its last transaction.
const IDLE: Duration = Duration::from_secs(30);

/// The window the backends are counted over: the broker's per-tenant background work opens connections in waves, so
/// the lowest count of the window, not any one count, shows the backends were released.
const WINDOW: Duration = Duration::from_secs(20);

/// Pins: once each of the scale tests' tenant count was written and read, through a broker keeping every tenant
/// resident behind a transaction pooler that closes a server 5 s after its last transaction, and then left idle for
/// 30 s, the broker reports exactly those tenants resident, or one more, and the client backends on tenant databases
/// fall below a quarter of the tenant count in the following 20 s
/// Clause: 5.7.1
/// Tenant: private (the subject is the runtime cache of many tenants)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`;
/// the extra resident tenant is the unnamed default
#[gtest]
#[tokio::test]
async fn p_scl_bkd_0001_an_idle_cache_of_every_tenant_releases_its_backends() -> Result<()> {
    let pooling = SuiteConfig::get()?.pooling()?;
    let wide = broker::quiet_client(BrokerRole::Wide)?;
    let set = TenantSet::provision(&wide, &Namespace::fresh(), pooling.scale_tenants(), CONCURRENCY).await?;
    for index in 0..set.len() {
        expect_that!(set.read(index).await.is_success(), eq(true), "tenant {index}");
    }
    sleep(IDLE).await;
    let resident = MetricsReading::of(BrokerRole::Wide).await?.sum(BrokerMetric::RuntimesCached);
    let series = BackendSampler::over(ServerBackends::of(BrokerRole::Wide)?, BackendScope::TenantDatabases, WINDOW).await?;
    println!("resident {resident:?}; tenant backends {series:?}");
    let touched = f64::from(u32::try_from(set.len())?);
    expect_that!(resident, some(all![ge(touched), le(touched + 1.0)]));
    verify_that!(series.trough, lt(u64::try_from(set.len() / 4)?))
}
