//! A registration update is notified against what the subscriber was told.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    clock_wait::wait_past,
    config::SuiteConfig,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A throttled subscription told of a first registration and held back from a second is notified of the
/// second's update as `newlyMatching` and then of the first's update as updated
/// Clause: 5.3.3, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_ntf_6608_a_registration_update_is_notified_against_what_the_subscriber_was_told() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_h_type = namespace.entity_type("VehicleH")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_h_type.as_str()])
        .throttling(10)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_h_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    sink.wait_for_at_least(1, 1).await?;
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_h_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let slept_until = (Utc::now() + TimeDelta::seconds(11)).timestamp();
    wait_past(slept_until).await;
    let fragment = RegistrationFragment::builder().endpoint(source.endpoint("changed")?).build()?;
    broker.registrations().update(&registration_2_id, &fragment).send().await?;
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    let notification_2 = notifications.get(1).or_fail()?.registrations()?;
    expect_that!(notification_2.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(
        notification_2.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_2_id))
    );
    let slept_until_2 = (Utc::now() + TimeDelta::seconds(11)).timestamp();
    wait_past(slept_until_2).await;
    let fragment_2 = RegistrationFragment::builder().endpoint(source.endpoint("changed")?).build()?;
    broker.registrations().update(&registration_id, &fragment_2).send().await?;
    sink.wait_for_at_least(1, 3).await?;
    let notifications_2 = sink.received(1).await?;
    let notification_3 = notifications_2.get(2).or_fail()?.registrations()?;
    expect_that!(notification_3.trigger_reason(), eq(TriggerReason::Updated));
    expect_that!(notification_3.data(), len(eq(1)));
    expect_that!(notification_3.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    Ok(())
}
