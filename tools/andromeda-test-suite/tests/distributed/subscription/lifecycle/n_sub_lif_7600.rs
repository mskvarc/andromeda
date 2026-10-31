//! Is active updates move the status between paused and active.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Representation},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: Update Subscription with `isActive` false answers 204 and the subscription reads status paused; then
/// `isActive` true answers 204 and status is active; then `isActive` true with a future `expiresAt` answers 204 and
/// status stays active
/// Clause: 5.8.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_lif_7600_is_active_updates_move_the_status_between_paused_and_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().activation(Activation::Paused).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Paused)));
    let fragment_2 = SubscriptionFragment::builder().activation(Activation::Active).build()?;
    broker.subscriptions().update(&subscription_id, &fragment_2).send().await?;
    let subscription_3 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    let fragment_3 = SubscriptionFragment::builder()
        .activation(Activation::Active)
        .expires_at("2999-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment_3).send().await?;
    let subscription_4 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_4.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(subscription_4.expires_at(), some(eq(&"2999-01-01T00:00:00Z".parse::<DateTime>()?)));
    Ok(())
}
