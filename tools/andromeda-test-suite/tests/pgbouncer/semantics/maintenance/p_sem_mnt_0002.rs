//! Pooled requests are unaffected while a repack runs.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, BrokerMetricsError, MetricLabel, MetricsReading},
    config::BrokerRole,
    load_run::run_load,
    maintenance_seed::{CURRENT_ATTRIBUTES_REPACK, MaintenanceSeed},
    namespace::Namespace,
    outcome::Done,
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, EntityId};
use std::{result, time::Duration};

/// The tenants written and read.
const TENANTS: usize = 4;

/// The workers of the load.
const WORKERS: usize = 16;

/// How long one round of the load runs.
const ROUND: Duration = Duration::from_secs(20);

/// The most rounds the load runs while it waits for the repack: the next free-space probe queues it, the repack
/// task, every 15 s, runs it. The rounds end well inside the ten minutes the profile gives a pgbouncer test.
const ROUNDS: usize = 18;

/// The entities written to bloat `current_attributes`, in batches of [`BATCH`]: their attributes take about 85 MiB,
/// past the responsive profile's 64 MiB repack size floor.
const ENTITIES: usize = 80_000;

/// The entities one batch creates or deletes.
const BATCH: usize = 1_000;

/// The properties each bloating entity carries.
const PROPERTIES: usize = 6;

/// Pins: in a broker running automatic maintenance and automatic repacks with the responsive profile behind a
/// transaction pooler, a tenant whose `current_attributes` 72,000 deleted entities of 6 properties each left free
/// is repacked online while 16 workers send a Batch Entity Upsert, an Update Attributes or a Retrieve Entity in turn
/// over 4 tenants, every request is answered with a success, and the broker counts the repack
/// Clause: 5.6.8, 5.6.3, 5.7.1, 5.6.9
/// Tenant: private (the subject is the maintenance of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and repacks a table through two sessions of its own on direct connections
#[gtest]
#[tokio::test]
async fn p_sem_mnt_0002_pooled_requests_are_unaffected_while_a_repack_runs() -> Result<()> {
    let pooled = broker::quiet_client(BrokerRole::PooledDurable)?;
    let namespace = Namespace::fresh();
    let set = TenantSet::provision(&pooled, &namespace, TENANTS, TENANTS).await?;
    let bloated = set.member(0);
    let entity_type = namespace.entity_type("Bloat")?;
    let ids = (0..ENTITIES)
        .map(|index| namespace.urn("Bloat", index))
        .collect::<result::Result<Vec<EntityId>, _>>()?;
    for chunk in ids.chunks(BATCH) {
        let entities = chunk
            .iter()
            .map(|id| {
                (0..PROPERTIES)
                    .fold(Entity::builder().id(id.clone()).entity_type(entity_type.clone()), |builder, index| {
                        builder.property(format!("p{index}"), "v".repeat(64))
                    })
                    .build()
            })
            .collect::<result::Result<Vec<_>, _>>()?;
        bloated.client.entities().batch_create(&entities).send().await?.done()?;
    }
    let deleted = ids.len() * 9 / 10;
    for chunk in ids[..deleted].chunks(BATCH) {
        bloated.client.entities().batch_delete(chunk).send().await?.done()?;
    }
    MaintenanceSeed::of(BrokerRole::PooledDurable)?
        .seed_breach(&bloated.tenant, CURRENT_ATTRIBUTES_REPACK)
        .await?;
    let before = repacks().await?;
    let picker = UniformPicker::new(set.len(), 11)?;
    let mut unsuccessful = 0;
    let mut after = before;
    for _ in 0..ROUNDS {
        let report = run_load(WORKERS, ROUND, |number| {
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
        println!("{report}");
        unsuccessful += report.unsuccessful();
        after = repacks().await?;
        if after > before {
            break;
        }
    }
    println!("repack actions {before} then {after}");
    expect_that!(unsuccessful, eq(0));
    verify_that!(after, gt(before))
}

/// The repacks the broker counted, over every outcome; none before the first.
async fn repacks() -> result::Result<f64, BrokerMetricsError> {
    Ok(MetricsReading::of(BrokerRole::PooledDurable)
        .await?
        .sum_labelled(BrokerMetric::MaintenanceActions, MetricLabel::Action, "repack")
        .unwrap_or_default())
}
