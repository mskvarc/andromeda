//! A future expires at is stored and an absent one leaves the subscription perpetual.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: Create Subscription with an `expiresAt` in the future reads back with that `expiresAt` and status active,
/// and one without `expiresAt` reads back with no `expiresAt` and status active
/// Clause: 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6802_a_future_expires_at_is_stored_and_an_absent_one_leaves_the_subscription_perpetual() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2999-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.expires_at(), some(eq(&"2999-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Active)));
    let subscription_2_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2_2).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(subscription_3.expires_at(), none());
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    Ok(())
}
