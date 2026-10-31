//! Is active true resumes a paused subscription.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: a subscription created with `isActive` false reports status paused, and a fragment with `isActive` true
/// and no `expiresAt` brings it back to status active
/// Clause: 5.8.2.4, 5.8.1.4, 5.2.12, 5.5.8
#[gtest]
#[tokio::test]
async fn n_sub_lif_0001_is_active_true_resumes_a_paused_subscription() -> Result<()> {
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
    let fragment = SubscriptionFragment::builder().activation(Activation::Active).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    Ok(())
}
