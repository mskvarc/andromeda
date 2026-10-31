//! Updating and deleting a subscription under a snapshot changes its delivery.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: A Subscription created with NGSILD-Snapshot and set inactive is not notified of an Entity created under
/// the Snapshot, is notified of the next one once set active again, and after its deletion is not notified while a
/// second Subscription of the Snapshot is
/// Clause: 5.5.15, 5.8.2.4, 5.8.5.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_snp_qry_8117_updating_and_deleting_a_subscription_under_a_snapshot_changes_its_delivery() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_inactive_id = namespace.urn("Vehicle", "inactive")?;
    let vehicle_active_id = namespace.urn("Vehicle", "active")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_deleted_id = namespace.urn("Vehicle", "deleted")?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.on_snapshot(&snapshot_id).subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().activation(Activation::Paused).build()?;
    broker
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .update(&subscription_id, &fragment)
        .send()
        .await?;
    let vehicle_inactive = Entity::builder()
        .id(vehicle_inactive_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_inactive).send().await?.done()?;
    let fragment_2 = SubscriptionFragment::builder().activation(Activation::Active).build()?;
    broker
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .update(&subscription_id, &fragment_2)
        .send()
        .await?;
    let vehicle_active = Entity::builder()
        .id(vehicle_active_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_active).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_active_id)));
    broker.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_id).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.on_snapshot(&snapshot_id).subscriptions().create(&subscription_2).send().await?;
    let vehicle_deleted = Entity::builder()
        .id(vehicle_deleted_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 3)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_deleted).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-deleted')]").parse()?,
    )];
    sink.wait_for_at_least_where(2, &selected, 1).await?;
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(1)));
    broker.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
