//! A source identity retrieval naming a snapshot is refused.

use andromeda_test_suite::{broker, namespace::Namespace, snapshot_refusal::refused_on_snapshot};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::RetrieveSourceIdentity,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Retrieve Context Source Identity Information with NGSILD-Snapshot naming an existing Snapshot answers 422
/// `OperationNotSupported` with the same header and a detail naming the core, temporal and entity-subscription
/// operations
/// Clause: 5.5.15, 6.3.22, 5.15.1
/// Recorded: clause 5.5.15 permits on a Snapshot the operations of the Core and Temporal APIs alone and names no error
/// for another; Andromeda answers `OperationNotSupported`
#[gtest]
#[tokio::test]
async fn o_snp_scp_8105_a_source_identity_retrieval_naming_a_snapshot_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let answer = broker
        .send_unchecked(broker.on_snapshot(&snapshot_id).unchecked(&RetrieveSourceIdentity)?)
        .await;
    expect_that!(&answer, refused_on_snapshot(&snapshot_id));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
