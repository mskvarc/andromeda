//! An inactive subscription reads back paused.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: Create Context Source Registration Subscription with `isActive` false answers 201, and the subscription
/// then reads back with `isActive` false and status paused
/// Clause: 5.11.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_cre_0003_an_inactive_subscription_reads_back_paused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .activation(Activation::Paused)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.activation(), eq(Activation::Paused));
    expect_that!(retrieved.output().status, some(eq(SubscriptionStatus::Paused)));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
