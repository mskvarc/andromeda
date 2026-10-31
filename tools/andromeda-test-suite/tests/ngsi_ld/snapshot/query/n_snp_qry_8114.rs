//! Subscription operations under a snapshot touch that snapshot alone.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::{NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Creates the Snapshot `id` over `vehicle_type` and waits until it has captured nothing.
async fn empty_snapshot(broker: &BrokerClient, id: &SnapshotId, vehicle_type: &str) -> Result<()> {
    let snapshot = Snapshot::builder()
        .id(id.clone())
        .query(QueryDocument::builder().entities([vehicle_type]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await
}

/// Pins: Create, retrieve, list, update and delete Subscription with NGSILD-Snapshot act on that Snapshot alone:
/// the Subscription created under one Snapshot reads under it with the same header, is listed there, is absent
/// under a second Snapshot and live, takes the update and is gone after the delete
/// Clause: 5.5.15, 6.3.22, 5.8.1.4, 5.8.2.4, 5.8.3.4, 5.8.4.4, 5.8.5.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8114_subscription_operations_under_a_snapshot_touch_that_snapshot_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    empty_snapshot(&broker, &snapshot_id, vehicle_type.as_str()).await?;
    empty_snapshot(&broker, &snapshot_2_id, vehicle_type.as_str()).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    let created = broker
        .with_responses()
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    expect_that!(
        created.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        retrieved.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    expect_that!(retrieved.outcome.id(), eq(&subscription_id));
    let under_sibling = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_2_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&under_sibling, answered(StatusCode::NOT_FOUND));
    let live = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&live, answered(StatusCode::NOT_FOUND));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .query()
        .limit(1000)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&subscription_id)));
    let fragment = SubscriptionFragment::builder().activation(Activation::Paused).build()?;
    broker
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .update(&subscription_id, &fragment)
        .send()
        .await?;
    let paused = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(paused.activation(), eq(Activation::Paused));
    broker.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_id).send().await?;
    let gone = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&gone, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
