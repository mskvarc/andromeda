//! Removing expires at keeps an expired subscription expired.

use andromeda_test_suite::{broker, eventually::retried, instant_ahead::seconds_ahead, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember, SubscriptionStatus},
};

/// Pins: a fragment setting `expiresAt` to the NGSI-LD Null on an expired subscription removes the `expiresAt` and
/// leaves the status expired
/// Clause: 5.8.2.4, 5.8.1.4
///
/// The expiry is an instant five seconds ahead on the runner's clock, which the broker shares.
#[gtest]
#[tokio::test]
async fn n_sub_lif_9003_removing_expires_at_keeps_an_expired_subscription_expired() -> Result<()> {
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
    let in_5_seconds = seconds_ahead(5).parse::<DateTime>()?;
    let expiring = SubscriptionFragment::builder().expires_at(in_5_seconds).build()?;
    broker.subscriptions().update(&subscription_id, &expiring).send().await?;
    retried(async || {
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(stored.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    let removal = SubscriptionFragment::builder().delete(SubscriptionMember::ExpiresAt).build()?;
    broker.subscriptions().update(&subscription_id, &removal).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.expires_at(), none());
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Expired)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
