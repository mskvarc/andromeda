//! The component report naming a snapshot is refused.

use andromeda_test_suite::{broker, namespace::Namespace, snapshot_refusal::refused_on_snapshot, wire::andromeda_request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::NgsiLdSnapshot,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: GET /ngsi-ld/andromeda/v1/version with NGSILD-Snapshot naming an existing Snapshot answers 422
/// `OperationNotSupported` with the same header and a detail naming the core, temporal and entity-subscription
/// operations
/// Clause: 5.5.15, 6.3.22
/// Recorded: Andromeda extension, not v1.9.1: the version endpoint reports the broker's components, which no Snapshot
/// holds
#[gtest]
#[tokio::test]
async fn a_snp_scp_8101_the_component_report_naming_a_snapshot_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    front.snapshots().create(&snapshot).send().await?;
    // The component report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request = andromeda_request_to(&front, Method::GET, "version")?.typed_header(NgsiLdSnapshot::try_new(snapshot_id.clone())?);
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, refused_on_snapshot(&snapshot_id));
    front.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
