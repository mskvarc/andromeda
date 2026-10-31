//! A change after expires at notifies only the unexpired subscription.

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
    http::headers::Accept,
    model::{DateTime, Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: Once a Subscription with `expiresAt` three seconds ahead reads back expired, two changes of its Entity
/// notify a Subscription without expiry and not it
/// Clause: 5.8.1.4, 5.8.6
///
/// A sentinel Subscription without an expiry is seen notifying at or after `expiresAt`, so the broker has passed
/// that instant; `notifiedAt` and `expiresAt` compare as instants.
#[gtest]
#[tokio::test]
async fn n_sub_lif_9002_a_change_after_expires_at_notifies_only_the_unexpired_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let expires_at = seconds_ahead(3).parse::<DateTime>()?;
    let vehicle_id = namespace.urn("Vehicle", "change")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let expiring_id = namespace.subscription_id("change-expiring")?;
    let sentinel_id = namespace.subscription_id("change-sentinel")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone());
    let expiring = Subscription::builder()
        .id(expiring_id.clone())
        .entities([selector.clone()])
        .expires_at(expires_at)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("change-expiring")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&expiring).send().await?;
    let sentinel = Subscription::builder()
        .id(sentinel_id.clone())
        .entities([selector])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("change-sentinel")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&sentinel).send().await?;
    retried(async || {
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&expiring_id)
            .send()
            .await?;
        verify_that!(stored.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    for speed in [2, 3] {
        let fragment = EntityFragment::builder().property("speed", speed).build()?;
        broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    }
    retried(async || {
        verify_that!(notified_since(&sink, "change-sentinel", &sentinel_id, &expires_at).await?, ge(2))?;
        Ok(())
    })
    .await?;
    expect_that!(notified_since(&sink, "change-expiring", &expiring_id, &expires_at).await?, eq(0));
    broker.subscriptions().delete(&sentinel_id).send().await?;
    broker.subscriptions().delete(&expiring_id).send().await?;
    Ok(())
}
