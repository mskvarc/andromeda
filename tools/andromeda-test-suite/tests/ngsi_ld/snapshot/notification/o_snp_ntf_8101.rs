//! A snapshot notification is json ld with its context and no ready flag.

use andromeda_test_suite::{broker, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use http::header::CONTENT_TYPE;
use ngsi_ld::snapshot::{QueryDocument, Snapshot, SnapshotId};

/// Pins: The `SnapshotNotification` of a Snapshot of the default Tenant is sent as application/ld+json without an
/// NGSILD-Tenant header, with an id under `urn:ngsi-ld:SnapshotNotification`:, an @context member and no
/// `snapshotReady` member
/// Clause: 5.16.6, 5.3.4, 6.3.6
/// Recorded: Table 5.3.4-1 defines no `snapshotReady` member beside the one its `expiresAt` row names for a deleted
/// Snapshot, and clause 5.16.6 leaves the media type to the binding; Andromeda sends application/ld+json with the
/// @context in the body and no `snapshotReady`
#[gtest]
#[tokio::test]
async fn o_snp_ntf_8101_a_snapshot_notification_is_json_ld_with_its_context_and_no_ready_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint(1)?.as_str())
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let headers = [HeaderCondition::exactly(CONTENT_TYPE, "^application/ld\\+json$")?];
    let notifications = sink.wait_for_at_least_matching(1, &[], &headers, 1).await?;
    let received = notifications.first().or_fail()?;
    expect_that!(received.headers.contains_key("NGSILD-Tenant"), eq(false));
    // The reader refuses every member outside Table 5.3.4-1, `snapshotReady` among them, so the read pins its absence.
    let notification = received.snapshot()?;
    expect_that!(notification.id().to_string(), starts_with("urn:ngsi-ld:SnapshotNotification:"));
    expect_that!(notification.context(), some(anything()));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
