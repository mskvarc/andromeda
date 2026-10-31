//! Notified at carries at most six fractional digits.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The `notifiedAt` of a `ContextSourceNotification` carries at most six fractional-second digits and ends in
/// Z
/// Clause: Table 5.3.2-1, 4.6.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_9011_notified_at_carries_at_most_six_fractional_digits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(600)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.notified_at().fraction_digits(), le(6));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
