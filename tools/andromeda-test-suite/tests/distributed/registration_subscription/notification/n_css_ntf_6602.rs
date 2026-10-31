//! A registration updated into the selection is first notified as newly matching.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::ContentType,
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Notification delivered as application/ld+json for a registration updated from another
/// entity type to the subscribed one is the first notification of the subscription, has `triggerReason`
/// `newlyMatching` and holds the registration alone in data
/// Clause: 5.11.7, 5.3.2, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_6602_a_registration_updated_into_the_selection_is_first_notified_as_newly_matching() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_t_type = namespace.entity_type("VehicleT")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_o_type = namespace.entity_type("VehicleO")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_t_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_o_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_t_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().update(&registration_id, &fragment).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification_head = notifications.first().or_fail()?.head();
    expect_that!(
        notification_head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    Ok(())
}
