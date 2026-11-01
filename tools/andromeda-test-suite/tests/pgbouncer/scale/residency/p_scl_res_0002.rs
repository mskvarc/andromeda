//! A Zipf working set keeps its hottest tenants resident.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricsReading},
    config::{BrokerRole, SuiteConfig},
    load_run::run_load,
    namespace::Namespace,
    tenant_picker::ZipfPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;

/// The workers of the Zipf warm-up.
const WARM_WORKERS: usize = 48;

/// The workers of the hot window.
const HOT_WORKERS: usize = 16;

/// The hottest tenants the hot window reads.
const HOT_TENANTS: usize = 16;

/// How long the hot window lasts.
const HOT_WINDOW: Duration = Duration::from_secs(10);

/// The first writes sent at once while the tenants are brought up.
const CONCURRENCY: usize = 32;

/// Pins: 48 workers retrieving the Device of tenants picked by a Zipf law of exponent 1.1 over the scale tests' tenant
/// count for the load duration, then 16 workers retrieving those of the 16 hottest for 10 s, have every request
/// answered with a success, and the broker builds fewer than 16 tenants cold during the hot window
/// Clause: 5.7.1
/// Tenant: private (the subject is the runtime cache of many tenants)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and keeps at most 32 tenants resident, evicting the least recently used
#[gtest]
#[tokio::test]
async fn p_scl_res_0002_a_zipf_working_set_keeps_its_hottest_tenants_resident() -> Result<()> {
    let pooling = SuiteConfig::get()?.pooling()?;
    let zipf = broker::quiet_client(BrokerRole::Zipf)?;
    let set = TenantSet::provision(&zipf, &Namespace::fresh(), pooling.scale_tenants(), CONCURRENCY).await?;
    let picker = ZipfPicker::new(u16::try_from(set.len())?, 1.1, 0xB0B)?;
    let warm = run_load(WARM_WORKERS, pooling.load_duration(), |_| set.read(picker.pick())).await;
    let cold_before = MetricsReading::of(BrokerRole::Zipf).await?.sum(BrokerMetric::ColdBuilds);
    let hot = run_load(HOT_WORKERS, HOT_WINDOW, |number| set.read(number % HOT_TENANTS)).await;
    let cold_after = MetricsReading::of(BrokerRole::Zipf).await?.sum(BrokerMetric::ColdBuilds);
    println!("warm-up {warm}; hot window {hot}; cold builds {cold_before:?} then {cold_after:?}");
    expect_that!(warm.unsuccessful(), eq(0));
    expect_that!(hot.unsuccessful(), eq(0));
    let hot_builds = cold_after.unwrap_or_default() - cold_before.unwrap_or_default();
    verify_that!(hot_builds, lt(16.0))
}
