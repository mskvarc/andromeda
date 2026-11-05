//! A periodic subscription stops notifying at its expires at.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    instant_ahead::seconds_ahead,
    namespace::Namespace,
    notification_sink::NotificationSink,
    notified_since::notified_since,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DateTime, Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with `timeInterval` 1 and `expiresAt` six seconds ahead notifies before that instant and
/// never with a `notifiedAt` at or after it, while one without expiry keeps notifying past it
/// Clause: 5.8.1.4, 5.8.6
///
/// The window is read after the Entity exists, so the first request of a cold tenant cannot spend it. A sentinel Subscription without an expiry is seen notifying at or after `expiresAt`, so the broker has passed
/// that instant; `notifiedAt` and `expiresAt` compare as instants.
#[gtest]
#[tokio::test]
async fn n_sub_lif_9001_a_periodic_subscription_stops_notifying_at_its_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", "periodic")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let expiring_id = namespace.subscription_id("periodic-expiring")?;
    let sentinel_id = namespace.subscription_id("periodic-sentinel")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let expires_at = seconds_ahead(6).parse::<DateTime>()?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id);
    let expiring = Subscription::builder()
        .id(expiring_id.clone())
        .entities([selector.clone()])
        .time_interval(1)
        .expires_at(expires_at)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic-expiring")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&expiring).send().await?;
    let sentinel = Subscription::builder()
        .id(sentinel_id.clone())
        .entities([selector])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic-sentinel")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&sentinel).send().await?;
    retried(async || {
        verify_that!(notified_since(&sink, "periodic-sentinel", &sentinel_id, &expires_at).await?, ge(2))?;
        Ok(())
    })
    .await?;
    expect_that!(sink.received("periodic-expiring").await?, not(is_empty()));
    expect_that!(notified_since(&sink, "periodic-expiring", &expiring_id, &expires_at).await?, eq(0));
    broker.subscriptions().delete(&sentinel_id).send().await?;
    broker.subscriptions().delete(&expiring_id).send().await?;
    Ok(())
}
