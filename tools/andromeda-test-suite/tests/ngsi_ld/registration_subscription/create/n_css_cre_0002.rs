//! A subscription without is active reads back active.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: Create Context Source Registration Subscription without `isActive` answers 201, and the subscription then
/// reads back with status active
/// Clause: 5.11.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_cre_0002_a_subscription_without_is_active_reads_back_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
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
    expect_that!(retrieved.output().status, some(eq(SubscriptionStatus::Active)));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
