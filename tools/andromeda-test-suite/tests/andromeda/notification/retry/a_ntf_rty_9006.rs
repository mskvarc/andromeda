//! A live notification carries a random id, different on every send.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, notification_uuid::uuid_version, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use uuid::Version;

/// Pins: under live delivery, two Entity creations matched by one Subscription reach its endpoint under two different
/// notification `id`s, both random (version 4) UUIDs
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: the notification `id` is random per notification under live delivery and
/// stable per queued notification under durable delivery
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9006_a_live_notification_carries_a_random_id_different_on_every_send() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_live = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_live.subscriptions().create(&subscription).send().await?;
    for n in 1..=2 {
        let entity = Entity::builder()
            .id(namespace.urn("Building", n)?)
            .entity_type(building_type.clone())
            .property("name", "a")
            .build()?;
        broker_live.entities().create(&entity).send().await?.done()?;
    }
    let received = sink.wait_for_at_least(1, 2).await?;
    let ids = received
        .iter()
        .map(|notification| Ok(notification.entities(Lossless::Normalized)?.id().clone()))
        .collect::<Result<Vec<_>>>()?;
    expect_that!(ids.len(), eq(2));
    let first_id = ids.first().map(ToString::to_string);
    expect_that!(ids.get(1).map(ToString::to_string), not(eq(&first_id)));
    for id in &ids {
        expect_that!(uuid_version(id), some(eq(Version::Random)));
    }
    broker_live.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
