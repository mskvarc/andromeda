//! A snapshot executing no query is failure and still scopes operations.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::NgsiLdMediaType,
    snapshot::{Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot created with an empty `snapshotQueries` executes no query and ends with `snapshotStatus`
/// failure, and Query Entities under it answers 200, nothing, and the NGSILD-Snapshot header
/// Clause: 5.16.1.4, 6.3.22
#[gtest]
#[tokio::test]
async fn n_snp_cre_8112_a_snapshot_executing_no_query_is_failure_and_still_scopes_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .queries([])
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("failure".parse::<SnapshotStatus>()?))?;
        verify_that!(snapshot_2.output().queries_details, is_empty())?;
        Ok(())
    })
    .await?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(
        headed.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    expect_that!(page.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
