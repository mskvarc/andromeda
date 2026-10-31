//! A subscription with a future expires at reads back active.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: A Context Source Registration Subscription created with an `expiresAt` in the future reads back with
/// status active and that `expiresAt`
/// Clause: 5.2.12, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_cre_0007_a_subscription_with_a_future_expires_at_reads_back_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(subscription_2.expires_at(), some(eq(&"2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    Ok(())
}
