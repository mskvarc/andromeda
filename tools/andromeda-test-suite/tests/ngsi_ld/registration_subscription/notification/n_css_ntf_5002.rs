//! Each registration created is notified once and alone.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: each Context Source Registration created while a Context Source Registration Subscription selects it is
/// one change of a matching registration and is notified exactly once, as `newlyMatching` with that registration
/// alone, in creation order
/// Clause: 5.11.2.4, 5.11.7, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_5002_each_registration_created_is_notified_once_and_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str(), bus_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let selected = [BodyPattern::Selects("$[?(@.triggerReason=='newlyMatching')]".parse()?)];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(bus_type.clone()).with_id_pattern(namespace.id_pattern("Bus")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let selected_2 = [BodyPattern::Selects("$[?(@.triggerReason=='newlyMatching')]".parse()?)];
    sink.wait_for_at_least_where(1, &selected_2, 2).await?;
    // the barrier: a change to the first registration is the next notification, so a duplicate of either creation has arrived before it
    let fragment = RegistrationFragment::builder().endpoint(source.endpoint("1-changed")?).build()?;
    broker.registrations().update(&registration_id, &fragment).send().await?;
    let selected_3 = [BodyPattern::Selects("$[?(@.triggerReason=='updated')]".parse()?)];
    sink.wait_for_at_least_where(1, &selected_3, 1).await?;
    let selected_4 = [BodyPattern::Selects("$[?(@.triggerReason=='newlyMatching')]".parse()?)];
    let notifications = sink.received_where(1, &selected_4).await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    let notification_2 = notifications.get(1).or_fail()?.registrations()?;
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(
        notification_2.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_2_id))
    );
    let selected_5 = [BodyPattern::Selects("$[?(@.triggerReason=='updated')]".parse()?)];
    let notifications_2 = sink.received_where(1, &selected_5).await?;
    expect_that!(notifications_2, len(eq(1)));
    Ok(())
}
