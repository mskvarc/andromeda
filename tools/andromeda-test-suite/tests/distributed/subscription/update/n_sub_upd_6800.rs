//! A partial update keeps the unmentioned members and a future expires at leaves an active subscription active.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: Update Subscription with a description alone answers 204 and keeps the stored q; a later fragment with
/// only a future `expiresAt` answers 204 and the subscription is still active with that `expiresAt`
/// Clause: 5.8.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_6800_a_partial_update_keeps_the_unmentioned_members_and_a_future_expires_at_leaves_an_active_subscription_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .description("original")
        .entities([vehicle_type.as_str()])
        .q(q::attr("speed").gt(10).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().description("changed").build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.description(), some(eq("changed")));
    expect_that!(subscription_2.q().map(ToString::to_string), some(eq("speed>10")));
    let fragment_2 = SubscriptionFragment::builder().expires_at("2999-01-01T00:00:00Z".parse()?).build()?;
    broker.subscriptions().update(&subscription_id, &fragment_2).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(subscription_3.expires_at(), some(eq(&"2999-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(subscription_3.description(), some(eq("changed")));
    Ok(())
}
