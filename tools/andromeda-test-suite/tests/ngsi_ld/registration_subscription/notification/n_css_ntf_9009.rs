//! A paused periodic csource subscription is silent.

use andromeda_test_suite::{broker, config::SuiteConfig, entity_registration::entity_registration, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    subscription::{Activation, Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription with `timeInterval` 1 and `isActive` false receives no
/// notification while an active one with the same interval, created after it, receives three
/// Clause: Table 5.2.12-2, 5.11.7, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_ntf_9009_a_paused_periodic_csource_subscription_is_silent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern_ending(".+", ".*")?,
        config.unreachable_origin().as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(1)
        .activation(Activation::Paused)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .time_interval(1)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    sink.wait_for_at_least(2, 3).await?;
    expect_that!(sink.received(1).await?, is_empty());
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registration_subscriptions().delete(&subscription_2_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
