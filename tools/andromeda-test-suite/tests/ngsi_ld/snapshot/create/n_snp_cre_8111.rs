//! A snapshot whose queries all succeed without a result is empty.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot of two successfully executed queries without a result ends with `snapshotStatus` empty, and
/// Query Entities under it answers nothing
/// Clause: 5.16.1.4, 5.2.42
#[gtest]
#[tokio::test]
async fn n_snp_cre_8111_a_snapshot_whose_queries_all_succeed_without_a_result_is_empty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_type = namespace.entity_type("Absent")?;
    let absent_too_type = namespace.entity_type("AbsentToo")?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([absent_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([absent_too_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        verify_that!(
            snapshot_2.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        verify_that!(
            snapshot_2.output().queries_details.get(1).map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        Ok(())
    })
    .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
