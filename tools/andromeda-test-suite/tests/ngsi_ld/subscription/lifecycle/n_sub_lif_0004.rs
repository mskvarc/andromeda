//! Is active true with a future expires at reactivates an expired subscription.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: a subscription whose `expiresAt` has passed reports status expired, and a fragment with `isActive` true
/// and an `expiresAt` in the future brings it back to status active
/// Clause: 5.8.2.4, 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_lif_0004_is_active_true_with_a_future_expires_at_reactivates_an_expired_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let in_5_seconds: DateTime = (Utc::now() + TimeDelta::seconds(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let fragment = SubscriptionFragment::builder().expires_at(in_5_seconds).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    retried(async || {
        let subscription_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    let fragment_2 = SubscriptionFragment::builder()
        .activation(Activation::Active)
        .expires_at("2099-08-01T22:07:00Z".parse()?)
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment_2).send().await?;
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
