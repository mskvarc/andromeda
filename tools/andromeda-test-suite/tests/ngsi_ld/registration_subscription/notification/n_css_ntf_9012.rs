//! A csf on mode notifies only the exclusive registration.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription whose csf is mode=="exclusive" is not notified of an inclusive
/// registration of the type and is notified `newlyMatching` of an exclusive one
/// Clause: 4.9, 5.11.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_css_ntf_9012_a_csf_on_mode_notifies_only_the_exclusive_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_inclusive_id = namespace.registration_id("inclusive")?;
    let registration_exclusive_id = namespace.registration_id("exclusive")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .csf(q::attr("mode").eq("exclusive").build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration_inclusive = CSourceRegistration::builder(source.endpoint("inclusive")?)
        .id(registration_inclusive_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    broker.registrations().register(&registration_inclusive).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_inclusive_cleanup = Cleanup::of(&broker).registration(&registration_inclusive_id);
    let registration_exclusive = CSourceRegistration::builder(source.endpoint("exclusive")?)
        .id(registration_exclusive_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_exclusive).send().await?;
    let _registration_exclusive_cleanup = Cleanup::of(&broker).registration(&registration_exclusive_id);
    // One notification, of the registration created second: the first sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(
        notification.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_exclusive_id))
    );
    Ok(())
}
