//! A snapshot operation naming a snapshot is refused and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, snapshot_refusal::refused_on_snapshot};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CloneSnapshot, CreateSnapshot, DeleteSnapshot, PurgeSnapshots, RetrieveSnapshot},
    },
    model::{NgsiLdMediaType, Uri},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotClone, SnapshotId},
};

/// Pins: Create Snapshot, Clone Snapshot, Purge Snapshots selecting an existing Snapshot by id, Retrieve Snapshot
/// Status and Delete Snapshot, each with NGSILD-Snapshot naming that Snapshot, answer 422 `OperationNotSupported` with
/// the same header and a detail naming the core, temporal and entity-subscription operations; neither the created
/// nor the cloned Snapshot exists and the named Snapshot remains
/// Clause: 5.5.15, 6.3.22, 5.16.1, 5.16.2, 5.16.3, 5.16.5, 5.16.7
/// Recorded: clause 5.5.15 permits on a Snapshot the operations of the Core and Temporal APIs alone and names no error
/// for another; Andromeda answers `OperationNotSupported`
#[gtest]
#[tokio::test]
async fn o_snp_scp_8102_a_snapshot_operation_naming_a_snapshot_is_refused_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let created_id = format!("urn:ngsi-ld:Snapshot:{namespace}-created").parse::<SnapshotId>()?;
    let cloned_id = format!("urn:ngsi-ld:Snapshot:{namespace}-cloned").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let created = Snapshot::builder()
        .id(created_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    let clone = SnapshotClone::builder().id(cloned_id.clone()).build()?;
    let selecting = q::attr("id")
        .eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<Uri>()?)?)
        .build()?;
    let on_snapshot = broker.on_snapshot(&snapshot_id);
    for request in [
        on_snapshot.unchecked(&CreateSnapshot { snapshot: &created })?,
        on_snapshot.unchecked(&CloneSnapshot {
            id: &snapshot_id,
            clone: &clone,
        })?,
        on_snapshot.unchecked(&PurgeSnapshots { q: &selecting })?,
        on_snapshot.unchecked(&RetrieveSnapshot { id: &snapshot_id })?,
        on_snapshot.unchecked(&DeleteSnapshot { id: &snapshot_id })?,
    ] {
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, refused_on_snapshot(&snapshot_id));
    }
    for absent_id in [&created_id, &cloned_id] {
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(absent_id)
            .send()
            .await;
        expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    }
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(remaining.id(), eq(&snapshot_id));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
