//! Registrations created before the update that selected them are reported once by it.

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
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Two registrations created while a subscription selected neither are not notified; the update of the
/// subscription is notified once as `newlyMatching` with both, and the next registration is the next notification
/// Clause: 5.11.2.4, 5.11.3.4, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_ntf_6606_registrations_created_before_the_update_that_selected_them_are_reported_once_by_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_q0_type = namespace.entity_type("VehicleQ0")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_q1_type = namespace.entity_type("VehicleQ1")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_q2_type = namespace.entity_type("VehicleQ2")?;
    let registration_3_id = namespace.registration_id(3)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_q0_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_q1_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_q2_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let fragment = SubscriptionFragment::registration_builder()
        .entities([vehicle_q1_type.as_str(), vehicle_q2_type.as_str()])
        .build()?;
    broker.registration_subscriptions().update(&subscription_id, &fragment).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(ge(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(2)));
    expect_that!(notification.data(), contains(has_id(&registration_id)));
    expect_that!(notification.data(), contains(has_id(&registration_2_id)));
    let registration_3 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_q1_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    sink.wait_for_at_least(1, 2).await?;
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(2)));
    let notification_2 = notifications_2.get(1).or_fail()?.registrations()?;
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(
        notification_2.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_3_id))
    );
    Ok(())
}
