//! Notified at carries at most six fractional digits.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::snapshot::{QueryDocument, Snapshot, SnapshotId};

/// Pins: The `notifiedAt` of a `SnapshotNotification` carries at most six fractional-second digits and ends in Z
/// Clause: 5.16.6, 4.6.3
#[gtest]
#[tokio::test]
async fn n_snp_ntf_8105_notified_at_carries_at_most_six_fractional_digits() -> Result<()> {
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
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.snapshot()?;
    expect_that!(notification.snapshot_id(), eq(&snapshot_id));
    expect_that!(
        notification.notified_at().to_string(),
        matches_regex(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,6})?Z$")
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
