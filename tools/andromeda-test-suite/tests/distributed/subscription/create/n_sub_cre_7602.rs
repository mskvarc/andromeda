//! Is active absent reads back active and false paused.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: Create Subscription on broker A without `isActive`, and another with `isActive` false, answers 201 each,
/// and they read back status active and paused
/// Clause: 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_7602_is_active_absent_reads_back_active_and_false_paused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
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
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .activation(Activation::Paused)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_3.output().status, some(eq(SubscriptionStatus::Active)));
    let subscription_4 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_2_id).send().await?;
    expect_that!(subscription_4.output().status, some(eq(SubscriptionStatus::Paused)));
    Ok(())
}
