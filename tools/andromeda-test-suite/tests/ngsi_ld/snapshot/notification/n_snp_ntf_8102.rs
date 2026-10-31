//! An update of a snapshot with an endpoint is notified.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    namespace::Namespace,
    notification_sink::{NotificationSink, ReceivedNotification},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId, SnapshotStatus},
};

/// Pins: Update Snapshot Status of a Snapshot with an endpoint (`snapshotPriority` 9, `snapshotLifetime` PT2H) is
/// followed by a `SnapshotNotification` carrying the new `snapshotPriority` and the `expiresAt` the update answered
/// Clause: 5.16.6, 5.3.4, 5.16.4.4
#[gtest]
#[tokio::test]
async fn n_snp_ntf_8102_an_update_of_a_snapshot_with_an_endpoint_is_notified() -> Result<()> {
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
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    sink.wait_for_at_least(1, 1).await?;
    let fragment = SnapshotFragment::builder().priority(9).lifetime("PT2H".parse::<Duration>()?).build()?;
    let updated = broker.snapshots().update(&snapshot_id, &fragment).send().await?;
    let expires_at = updated.output().expires_at;
    let notifications = retried(async || {
        let received = sink.received(1).await?;
        let notifications = received.iter().map(ReceivedNotification::snapshot).collect::<Result<Vec<_>, _>>()?;
        verify_that!(
            notifications.iter().filter(|notification| notification.priority().into_inner() == 9).count(),
            ge(1)
        )?;
        Ok(notifications)
    })
    .await?;
    let notification = notifications.iter().find(|notification| notification.priority().into_inner() == 9).or_fail()?;
    expect_that!(notification.expires_at(), eq(&expires_at));
    expect_that!(notification.snapshot_id(), eq(&snapshot_id));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
