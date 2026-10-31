//! A clone copies no subscription of its source.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, Input, Lossless, NgsiLdMediaType, Property, Representation},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: A clone of a Snapshot holding a delivered and an undeliverable Subscription lists no Subscription, a write
/// to the clone notifies none of the source's Subscriptions, and a Subscription under the source's Subscription id
/// can be created in the clone
/// Clause: 5.16.2.4, 5.5.15, 5.8.6
#[gtest]
#[tokio::test]
async fn n_snp_cln_8106_a_clone_copies_no_subscription_of_its_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let car_type = namespace.entity_type("Car")?;
    let first_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let second_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let subscription_1_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let car = |suffix: &str, speed: i32| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(namespace.urn("Car", suffix)?)
            .entity_type(car_type.clone())
            .property("speed", speed)
            .build()?)
    };
    let captured = vec![car("captured-1", 1)?, car("captured-2", 2)?, car("captured-3", 3)?];
    broker.entities().batch_create(&captured).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(first_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &first_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    broker
        .on_snapshot(&first_id)
        .entities()
        .create(&car("source-write", 10)?)
        .send()
        .await?
        .done()?;
    let colour = EntityFragment::builder().property("colour", Property::builder("red")).build()?;
    broker
        .on_snapshot(&first_id)
        .entities()
        .append_attributes(&namespace.urn("Car", "captured-1")?, &colour)
        .send()
        .await?
        .done()?;
    let subscription = |id: &SubscriptionId, endpoint: &str| -> Result<Subscription<Input>> {
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([car_type.as_str()])
            .notification(
                NotificationParams::builder(Endpoint::builder(endpoint).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Normalized)
                    .build()?,
            )
            .build()?)
    };
    broker
        .on_snapshot(&first_id)
        .subscriptions()
        .create(&subscription(&subscription_1_id, sink.endpoint(1)?.as_str())?)
        .send()
        .await?;
    broker
        .on_snapshot(&first_id)
        .subscriptions()
        .create(&subscription(&subscription_2_id, config.unreachable_origin().join("notify")?.as_str())?)
        .send()
        .await?;
    broker.on_snapshot(&first_id).entities().create(&car("notified", 40)?).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    broker
        .snapshots()
        .clone_snapshot(&first_id, &SnapshotClone::builder().id(second_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &second_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let listed = broker.with_accept(Accept::any()).on_snapshot(&second_id).subscriptions().query().send().await?;
    expect_that!(listed.items(), is_empty());
    broker.on_snapshot(&second_id).entities().create(&car("unnotified", 41)?).send().await?.done()?;
    broker
        .on_snapshot(&second_id)
        .subscriptions()
        .create(&subscription(&subscription_1_id, sink.endpoint(2)?.as_str())?)
        .send()
        .await?;
    broker.on_snapshot(&second_id).entities().create(&car("barrier", 42)?).send().await?.done()?;
    let barrier = namespace.urn("Car", "barrier")?;
    retried(async || {
        let ids = sink
            .received(2)
            .await?
            .iter()
            .map(|received| received.entities(Lossless::Normalized))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .flat_map(|notification| notification.data().iter().map(|entity| entity.id.clone()).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        verify_that!(ids, contains(eq(&barrier)))?;
        Ok(())
    })
    .await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    broker.snapshots().delete(&second_id).send().await?;
    broker.snapshots().delete(&first_id).send().await?;
    Ok(())
}
