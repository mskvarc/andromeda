//! A subscription under a snapshot ignores live and sibling snapshot writes.

use andromeda_test_suite::{
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
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription created with NGSILD-Snapshot is not notified of a matching Entity created live or under
/// another Snapshot: it is notified once, of the Entity created under its own Snapshot afterwards
/// Clause: 5.5.15, 5.8.6
#[gtest]
#[tokio::test]
async fn n_snp_qry_8116_a_subscription_under_a_snapshot_ignores_live_and_sibling_snapshot_writes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_live_id = namespace.urn("Vehicle", "live")?;
    let vehicle_sibling_id = namespace.urn("Vehicle", "sibling")?;
    let vehicle_own_id = namespace.urn("Vehicle", "own")?;
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
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    retried(async || {
        let snapshot_4 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_2_id)
            .send()
            .await?;
        verify_that!(snapshot_4.status(), eq("empty".parse::<SnapshotStatus>()?))?;
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
    let vehicle_live = Entity::builder()
        .id(vehicle_live_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle_live).send().await?.done()?;
    let vehicle_sibling = Entity::builder()
        .id(vehicle_sibling_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker.on_snapshot(&snapshot_2_id).entities().create(&vehicle_sibling).send().await?.done()?;
    let vehicle_own = Entity::builder()
        .id(vehicle_own_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 3)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_own).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_own_id)));
    broker.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_id).send().await?;
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
