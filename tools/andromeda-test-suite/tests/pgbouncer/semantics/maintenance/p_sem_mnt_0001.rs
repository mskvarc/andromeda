//! Pooled requests are unaffected while maintenance runs.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricsReading},
    config::BrokerRole,
    load_run::run_load,
    namespace::Namespace,
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;

/// The tenants written and read.
const TENANTS: usize = 4;

/// The workers of the load.
const WORKERS: usize = 16;

/// How long the load runs: past the maintenance pass the batches make due.
const LOAD: Duration = Duration::from_secs(15);

/// Pins: in a broker running automatic maintenance with the responsive profile behind a transaction pooler, 16
/// workers sending a Batch Entity Upsert, an Update Attributes or a Retrieve Entity in turn over 4 tenants for 15 s
/// have every request answered with a success, and the broker's maintenance decisions do not fall
/// Clause: 5.6.8, 5.6.3, 5.7.1
/// Tenant: private (the subject is the maintenance of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and holds its maintenance session locks on direct connections
#[gtest]
#[tokio::test]
async fn p_sem_mnt_0001_pooled_requests_are_unaffected_while_maintenance_runs() -> Result<()> {
    let pooled = broker::quiet_client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &Namespace::fresh(), TENANTS, TENANTS).await?;
    let picker = UniformPicker::new(set.len(), 11)?;
    let decisions_before = MetricsReading::of(BrokerRole::PooledDurable).await?.sum(BrokerMetric::MaintenanceDecisions);
    let report = run_load(WORKERS, LOAD, |number| {
        let tenant = picker.pick();
        let set = &set;
        async move {
            match number % 3 {
                0 => set.upsert_batch(tenant).await,
                1 => set.update(tenant).await,
                _ => set.read(tenant).await,
            }
        }
    })
    .await;
    let decisions_after = MetricsReading::of(BrokerRole::PooledDurable).await?.sum(BrokerMetric::MaintenanceDecisions);
    println!("{report}; maintenance decisions {decisions_before:?} then {decisions_after:?}");
    expect_that!(report.unsuccessful(), eq(0));
    verify_that!(decisions_after.unwrap_or_default(), ge(decisions_before.unwrap_or_default()))
}
