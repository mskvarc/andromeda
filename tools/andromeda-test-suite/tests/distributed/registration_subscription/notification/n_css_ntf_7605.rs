//! A delivered notification sets times sent last success and status ok.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, eventually::retried, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: After a Context Source Notification for a registration created on broker A is delivered, Retrieve Context
/// Source Registration Subscription on broker A answers `timesSent` at least 1, `lastNotification` and
/// `lastSuccess`, and status ok
/// Clause: 5.11.7, 5.2.12, 5.2.14
#[gtest]
#[tokio::test]
async fn n_css_ntf_7605_a_delivered_notification_sets_times_sent_last_success_and_status_ok() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    sink.wait_for_at_least(1, 1).await?;
    retried(async || {
        let subscription_2 = broker
            .with_accept(Accept::any())
            .registration_subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription_2.notification().output().times_sent, some(ge(1)))?;
        verify_that!(subscription_2.notification().output().last_notification, some(anything()))?;
        verify_that!(subscription_2.notification().output().last_success, some(anything()))?;
        verify_that!(subscription_2.notification().output().status, some(eq(NotificationStatus::Ok)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
