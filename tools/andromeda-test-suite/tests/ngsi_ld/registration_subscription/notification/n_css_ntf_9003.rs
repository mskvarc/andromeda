//! A change driven csource subscription is silent while nothing matches.

use andromeda_test_suite::{broker, config::SuiteConfig, entity_registration::entity_registration, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output, Representation},
    registration::{CSourceRegistration, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription without `timeInterval` created while no registration matches is
/// not notified: the first notification it receives is the one the registration created afterwards owes it
/// Clause: 5.11.2.4, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_9003_a_change_driven_csource_subscription_is_silent_while_nothing_matches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
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
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern_ending(".+", ".*")?,
        config.unreachable_origin().as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
