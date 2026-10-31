//! Watched and notification attributes form the matching attribute set.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription with `watchedAttributes` and notification attributes is
/// notified of a registration naming an attribute drawn from the notification attributes, one with only
/// `watchedAttributes` of the registration naming the watched one and not of one naming another, and one without
/// attributes of a registration naming any
/// Clause: 5.12, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_ntf_6603_watched_and_notification_attributes_form_the_matching_attribute_set() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_u_type = namespace.entity_type("VehicleU")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_w_type = namespace.entity_type("VehicleW")?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let vehicle_n_type = namespace.entity_type("VehicleN")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_u_type.as_str()])
        .watched_attributes([namespace.attribute("watched")?.to_string()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes([namespace.attribute("notified")?.to_string()])
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id)
        .entities([vehicle_w_type.as_str()])
        .watched_attributes([namespace.attribute("watchedtwo")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::registration_builder()
        .id(subscription_3_id)
        .entities([vehicle_n_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_3).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_u_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([namespace.attribute("notified")?.to_string()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_w_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([namespace.attribute("other")?.to_string()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let registration_3 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_w_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([namespace.attribute("watchedtwo")?.to_string()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.registrations()?;
    expect_that!(notification_2.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(
        notification_2.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_3_id))
    );
    let registration_4 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_n_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&broker).registration(&registration_4_id);
    sink.wait_for_at_least(3, 1).await?;
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.registrations()?;
    expect_that!(notification_3.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_3.data(), len(eq(1)));
    expect_that!(
        notification_3.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_4_id))
    );
    Ok(())
}
