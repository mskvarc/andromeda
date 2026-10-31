//! Is active true with a future expires at resumes a paused subscription.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: a fragment with `isActive` true and an `expiresAt` in the future brings a paused subscription back to
/// status active and stores the new `expiresAt`
/// Clause: 5.8.2.4, 5.8.1.4, 5.2.12, 5.5.8
#[gtest]
#[tokio::test]
async fn n_sub_lif_0002_is_active_true_with_a_future_expires_at_resumes_a_paused_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .activation(Activation::Paused)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Paused)));
    let fragment = SubscriptionFragment::builder()
        .activation(Activation::Active)
        .expires_at("2099-08-01T22:07:00Z".parse()?)
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_3.expires_at().map(DateTime::timestamp),
        some(eq("2099-08-01T22:07:00.0Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    Ok(())
}
