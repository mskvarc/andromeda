//! An expired subscription notifies nothing while an active one does.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: A subscription whose `expiresAt` has passed reads back as expired and receives no notification of an
/// entity creation that a second, unexpired subscription on the same type receives
/// Clause: 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_lif_6800_an_expired_subscription_notifies_nothing_while_an_active_one_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let in_4_seconds: DateTime = (Utc::now() + TimeDelta::seconds(4)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at(in_4_seconds)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    retried(async || {
        let subscription_3 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(2, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(0)));
    Ok(())
}
