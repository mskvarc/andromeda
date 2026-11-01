//! A periodic csource notification carries its subscription and the matching registration.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription with `timeInterval` 2 notifies on creation and again when its
/// interval is reached, each csource notification naming the subscription and carrying the registration it matches
/// Clause: 5.11.2.4, 5.3.3, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_0001_a_periodic_csource_notification_carries_its_subscription_and_the_matching_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let _subscription_cleanup = Cleanup::of(&broker).registration_subscription(&subscription_id);
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(2)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    // The initial notification and the first periodic one (clause 5.11.2.4); more grid points can pass before the
    // read, so no exact count is asserted.
    let notifications = sink.wait_for_at_least(1, 2).await?;
    for received in &notifications {
        let notification = received.registrations()?;
        expect_that!(notification.subscription_id(), eq(&subscription_id));
        expect_that!(
            notification.data().iter().map(CSourceRegistration::<Output>::id).collect::<Vec<_>>(),
            contains(points_to(eq(&registration_id)))
        );
    }
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
