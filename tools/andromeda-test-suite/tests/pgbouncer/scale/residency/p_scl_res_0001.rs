//! A working set three times the cache is served, shedding only paced 503s.

use andromeda_test_suite::{
    backend_sampler::BackendSampler,
    broker,
    broker_metrics::{BrokerMetric, MetricsReading},
    config::{BrokerRole, SuiteConfig},
    load_run::run_load,
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;

/// The workers sending requests at once.
const WORKERS: usize = 48;

/// The first writes sent at once while the tenants are brought up.
const CONCURRENCY: usize = 32;

/// The most client backends the broker may open on its server: the pooler's `max_user_connections` of 120 and the
/// broker's direct pools (its platform pool of 32, the migrations of its 8 concurrent loads and its maintenance),
/// with a margin.
const BACKEND_BUDGET: u64 = 200;

/// Pins: 48 workers sending a mix of seven Retrieve Entity, two Update Attributes and one Batch Entity Upsert in ten
/// over the scale tests' tenant count, three times the 32 tenants the broker keeps resident, for the scale tests'
/// load duration through a transaction pooler: every answer is a success or 503 naming a delay in `Retry-After`,
/// every request is answered, the 503s are under 5 % of the requests, the client backends on the server never pass
/// 200, and the broker built tenants cold during the run
/// Clause: 5.5.6, 5.7.1, 5.6.3, 5.6.8
/// Tenant: private (the subject is the runtime cache of many tenants)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and keeps at most 32 tenants resident
#[gtest]
#[tokio::test]
async fn p_scl_res_0001_a_working_set_three_times_the_cache_is_served_shedding_only_paced_503s() -> Result<()> {
    let pooling = SuiteConfig::get()?.pooling()?;
    let thrash = broker::quiet_client(BrokerRole::Thrash)?;
    let set = TenantSet::provision(&thrash, &Namespace::fresh(), pooling.scale_tenants(), CONCURRENCY).await?;
    let picker = UniformPicker::new(set.len(), 0x5EED)?;
    let cold_before = MetricsReading::of(BrokerRole::Thrash).await?.sum(BrokerMetric::ColdBuilds);
    let sampler = BackendSampler::start(ServerBackends::of(BrokerRole::Thrash)?, BackendScope::WholeServer);
    let report = run_load(WORKERS, pooling.load_duration(), |number| {
        let tenant = picker.pick();
        let set = &set;
        async move {
            match number % 10 {
                0..=6 => set.read(tenant).await,
                7 | 8 => set.update(tenant).await,
                _ => set.upsert_batch(tenant).await,
            }
        }
    })
    .await;
    let series = sampler.stop().await?;
    let cold_after = MetricsReading::of(BrokerRole::Thrash).await?.sum(BrokerMetric::ColdBuilds);
    println!("{report}; backends {series:?}; cold builds {cold_before:?} then {cold_after:?}");
    expect_that!(report.errors_besides_unavailability(), eq(0));
    expect_that!(report.unanswered(), eq(0));
    expect_that!(report.unpaced_unavailability(), eq(0));
    expect_that!(report.unsuccessful() * 20, lt(report.total()));
    expect_that!(series.peak, le(BACKEND_BUDGET));
    verify_that!(cold_after, some(gt(cold_before.unwrap_or_default())))
}
