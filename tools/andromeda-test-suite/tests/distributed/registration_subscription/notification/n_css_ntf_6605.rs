//! Paused and expired subscriptions are not notified.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, eventually::retried, namespace::Namespace, notification_sink::NotificationSink};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: A paused and an expired Context Source Registration Subscription receive no Context Source Notification
/// for a registration created once the expiry has passed, while an active one on the same type does
/// Clause: 5.11.7, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_6605_paused_and_expired_subscriptions_are_not_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_x_type = namespace.entity_type("VehicleX")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let registration_id = namespace.registration_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_x_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .activation(Activation::Paused)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    // Read after the first create, which a loaded broker can take seconds over, and truncated to the second, so the
    // expiry is still four seconds or more ahead when the second create is sent.
    let in_5_seconds: DateTime = (Utc::now() + TimeDelta::seconds(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_x_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at(in_5_seconds)
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::registration_builder()
        .id(subscription_3_id)
        .entities([vehicle_x_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_3).send().await?;
    retried(async || {
        let subscription_4 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registration_subscriptions()
            .retrieve(&subscription_2_id)
            .send()
            .await?;
        verify_that!(subscription_4.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_x_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    sink.wait_for_at_least(3, 1).await?;
    let notifications = sink.received(3).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(0)));
    let notifications_3 = sink.received(2).await?;
    expect_that!(notifications_3, len(eq(0)));
    Ok(())
}
