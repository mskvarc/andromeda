//! A subscription under a snapshot notifies with the snapshot header.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    header_condition::HeaderCondition,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription created with NGSILD-Snapshot is notified of an Entity created under the same Snapshot, and
/// the notification carries the NGSILD-Snapshot header naming it
/// Clause: 6.3.22, 5.5.15, 5.8.6
#[gtest]
#[tokio::test]
async fn n_snp_qry_8115_a_subscription_under_a_snapshot_notifies_with_the_snapshot_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
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
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let fields = [HeaderCondition::Is(
        HeaderName::from_static("ngsild-snapshot"),
        format!("urn:ngsi-ld:Snapshot:{namespace}-1"),
    )];
    let notifications = sink.wait_for_at_least_matching(1, &[], &fields, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_id)));
    broker.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
