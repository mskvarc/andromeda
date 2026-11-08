//! Snapshots settle through the pooler while other tenants are under load.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::retried,
    load_run::run_load,
    namespace::Namespace,
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};
use std::time::Duration;

/// The tenants that each take a Snapshot.
const SNAPSHOT_TENANTS: usize = 4;

/// The tenants the load spreads over.
const LOAD_TENANTS: usize = 16;

/// The workers of the load.
const WORKERS: usize = 16;

/// How long the load runs.
const LOAD: Duration = Duration::from_secs(20);

/// Pins: while 16 workers retrieve the Devices of 16 tenants for 20 s through a broker behind a transaction pooler,
/// a Snapshot of the Devices of each of 4 other tenants settles, and every request of the load is answered with a
/// success
/// Clause: 5.16.1.4, 5.7.1
/// Tenant: private (the subject is the branch schemas of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_sem_snp_0001_snapshots_settle_through_the_pooler_while_other_tenants_are_under_load() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::quiet_client(BrokerRole::PooledDurable)?;
    let snapshot_tenants = TenantSet::provision(&pooled, &Namespace::fresh(), SNAPSHOT_TENANTS, SNAPSHOT_TENANTS).await?;
    let load_tenants = TenantSet::provision(&pooled, &namespace, LOAD_TENANTS, LOAD_TENANTS).await?;
    let picker = UniformPicker::new(load_tenants.len(), 42)?;
    let device_type = namespace.entity_type("Device")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-load").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([device_type.as_str()]).build()?)
        .build()?;
    let snapshots = join_all(snapshot_tenants.members().map(|member| async {
        member.client.snapshots().create(&snapshot).send().await?;
        retried(async || {
            let read = member
                .client
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .snapshots()
                .retrieve(&snapshot_id)
                .send()
                .await?;
            verify_that!(read.status(), not(eq(SnapshotStatus::Preparing)))
        })
        .await
    }));
    let (report, settled) = tokio::join!(run_load(WORKERS, LOAD, |_| load_tenants.read(picker.pick())), snapshots);
    println!("{report}");
    for outcome in settled {
        outcome?;
    }
    verify_that!(report.unsuccessful(), eq(0))
}
