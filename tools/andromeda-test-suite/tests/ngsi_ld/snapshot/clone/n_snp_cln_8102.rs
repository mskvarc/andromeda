//! A clone is prepared in the background and independent of its source.

use andromeda_test_suite::{broker, namespace::Namespace, snapshot_capture::await_status};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotFragment, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: Clone Snapshot answers 201 with an empty body while the clone reads preparing or success, the clone ends
/// success, and an update of the source's `snapshotPriority` leaves the clone at 5
/// Clause: 5.16.2.4, 5.16.4.4
#[gtest]
#[tokio::test]
async fn n_snp_cln_8102_a_clone_is_prepared_in_the_background_and_independent_of_its_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &source_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    broker
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    let reading = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await?;
    expect_that!(reading.status().to_string(), any!(eq("preparing"), eq("success")));
    await_status(&broker, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let fragment = SnapshotFragment::builder().priority(2).build()?;
    let updated = broker.snapshots().update(&source_id, &fragment).send().await?;
    expect_that!(updated.priority(), some(eq(SnapshotPriority::try_new(2)?)));
    let clone = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await?;
    expect_that!(clone.priority(), some(eq(SnapshotPriority::try_new(5)?)));
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
