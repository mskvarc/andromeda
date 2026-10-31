//! Deleting purging or expiring a snapshot sends no notification.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::Uri,
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: A Snapshot with an endpoint sends its terminal `SnapshotNotification` and none when it is deleted, purged
/// or expires: after a later Snapshot's notification on the same endpoint, each of the three has sent exactly one
/// Clause: 5.16.6, 5.16.5.4, 5.16.7.4
/// Recorded: clause 5.16.6 has a `SnapshotNotification` sent after the preparation and after an update of the
/// status, and Table 5.3.4-1 describes one announcing a deletion without mandating it; Andromeda sends none on a
/// deletion, a purge or an expiry
#[gtest]
#[tokio::test]
async fn o_snp_ntf_8103_deleting_purging_or_expiring_a_snapshot_sends_no_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_deleted_id = format!("urn:ngsi-ld:Snapshot:{namespace}-deleted").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_expired_id = format!("urn:ngsi-ld:Snapshot:{namespace}-expired").parse::<SnapshotId>()?;
    let snapshot_barrier_id = format!("urn:ngsi-ld:Snapshot:{namespace}-barrier").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-deleted").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint(1)?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let selected = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-deleted')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    broker.snapshots().delete(&snapshot_deleted_id).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-purged").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint(1)?)
        .build()?;
    broker.snapshots().create(&snapshot_2).send().await?;
    let selected_2 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-purged')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_2, 1).await?;
    broker
        .snapshots()
        .purge(q::attr("id").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-purged").parse::<Uri>()?)?))
        .send()
        .await?;
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-expired").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT2S".parse()?)
        .endpoint(sink.endpoint(1)?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    let selected_3 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-expired')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_3, 1).await?;
    retried(async || {
        let answer = broker
            .with_responses()
            .with_accept(Accept::any())
            .snapshots()
            .retrieve(&snapshot_expired_id)
            .send()
            .await;
        verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;
        Ok(())
    })
    .await?;
    let snapshot_4 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-barrier").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint(1)?)
        .build()?;
    broker.snapshots().create(&snapshot_4).send().await?;
    let selected_4 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-barrier')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_4, 1).await?;
    let selected_5 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-deleted')]").parse()?,
    )];
    let notifications = sink.received_where(1, &selected_5).await?;
    expect_that!(notifications, len(eq(1)));
    let selected_6 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-purged')]").parse()?,
    )];
    let notifications_2 = sink.received_where(1, &selected_6).await?;
    expect_that!(notifications_2, len(eq(1)));
    let selected_7 = [BodyPattern::Selects(
        format!("$[?(@.snapshotId == 'urn:ngsi-ld:Snapshot:{namespace}-expired')]").parse()?,
    )];
    let notifications_3 = sink.received_where(1, &selected_7).await?;
    expect_that!(notifications_3, len(eq(1)));
    broker.snapshots().delete(&snapshot_barrier_id).send().await?;
    Ok(())
}
