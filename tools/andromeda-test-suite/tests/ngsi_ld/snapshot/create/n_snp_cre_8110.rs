//! A snapshot one query of which yields no result is partial.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot of two successfully executed queries, one over a type the broker holds an Entity of and one over a
/// type it holds none of, ends with `snapshotStatus` partial and the details success and empty
/// Clause: 5.16.1.4, 5.2.42
#[gtest]
#[tokio::test]
async fn n_snp_cre_8110_a_snapshot_one_query_of_which_yields_no_result_is_partial() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_type = namespace.entity_type("Absent")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 8)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([absent_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("partial".parse::<SnapshotStatus>()?))?;
        verify_that!(
            snapshot_2.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Success))
        )?;
        verify_that!(
            snapshot_2.output().queries_details.get(1).map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        Ok(())
    })
    .await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
