//! Execution details follow the order of the queries.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot of three queries (one by id with a result, one over a type without Entities, one by type with a
/// result) reports three `snapshotQueriesDetails` in the order of its queries: success, empty, success
/// Clause: 5.16.1.4, 5.2.42, Table 5.2.41-2
#[gtest]
#[tokio::test]
async fn n_snp_cre_8109_execution_details_follow_the_order_of_the_queries() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_type = namespace.entity_type("Absent")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(
            QueryDocument::builder()
                .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
                .build()?,
        )
        .query(QueryDocument::builder().entities([absent_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        verify_that!(snapshot_2.output().queries_details, len(eq(3)))?;
        verify_that!(
            snapshot_2.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Success))
        )?;
        verify_that!(
            snapshot_2.output().queries_details.get(1).map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        verify_that!(
            snapshot_2.output().queries_details.get(2).map(|details| details.result_status),
            some(eq(ExecutionStatus::Success))
        )?;
        Ok(())
    })
    .await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
