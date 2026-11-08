//! The Location of a Snapshot names its id percent-encoded as one path segment.

use andromeda_test_suite::{
    broker,
    followed_location::{identifier, request},
    namespace::Namespace,
    problem::answered,
    segment_identifier::segment_identifiers,
    snapshot_capture::await_status,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus};

/// Pins: Create Snapshot with an id holding /, ?, #, %41 or a non-ASCII character answers a Location ending in
/// /ngsi-ld/v1/snapshots/ and the id percent-encoded as one path segment, and a GET on that Location answers the
/// Snapshot with that id
/// Clause: 6.36.3.1, 6.37.3.1
/// Recorded: IETF RFC 3986 section 3.3 admits only pchar in a path segment, so the relative path names the created
/// Snapshot only with every other octet of its id percent-encoded, % included
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9136_the_location_of_a_snapshot_names_its_id_percent_encoded_as_one_path_segment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for segment_identifier in segment_identifiers(&namespace, "Snapshot") {
        let snapshot_id = SnapshotId::verbatim(&segment_identifier.id)?;
        let snapshot = Snapshot::builder()
            .id(snapshot_id.clone())
            .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
            .build()?;
        let created = broker.with_responses().snapshots().create(&snapshot).send().await?;
        expect_that!(
            header_value(created.head.headers(), &LOCATION),
            some(ends_with(format!("/ngsi-ld/v1/snapshots/{}", segment_identifier.segment)))
        );
        let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        expect_that!(identifier(&answer?)?, eq(snapshot_id.as_uri()));
        await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
        broker.snapshots().delete(&snapshot_id).send().await?;
    }
    Ok(())
}
