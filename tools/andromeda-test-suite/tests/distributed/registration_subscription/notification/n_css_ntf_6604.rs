//! A time interval subscription notifies initially and again with every matching registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription with `timeInterval` 2 over two registrations delivers an
/// initial and then a periodic Context Source Notification, each carrying both registrations
/// Clause: 5.11.7, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_6604_a_time_interval_subscription_notifies_initially_and_again_with_every_matching_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_p_type = namespace.entity_type("VehicleP")?;
    let registration_2_id = namespace.registration_id(2)?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_p_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_p_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_p_type.as_str()])
        .time_interval(2)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least(1, 2).await?;
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(2)));
    expect_that!(notification.data(), contains(has_id(&registration_id)));
    expect_that!(notification.data(), contains(has_id(&registration_2_id)));
    let notification_2 = notifications.get(1).or_fail()?.registrations()?;
    expect_that!(notification_2.data(), len(eq(2)));
    expect_that!(notification_2.data(), contains(has_id(&registration_id)));
    expect_that!(notification_2.data(), contains(has_id(&registration_2_id)));
    Ok(())
}
