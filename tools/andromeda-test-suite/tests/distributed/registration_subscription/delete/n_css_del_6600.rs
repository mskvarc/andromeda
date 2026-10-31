//! A deleted subscription is gone and no longer notified.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Context Source Registration Subscription answers 204 and the subscription then answers 404
/// `ResourceNotFound`, an unknown identifier answers 404 and a malformed one 400, and a registration created
/// afterwards notifies only the subscription that remains
/// Clause: 5.11.6, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_del_6600_a_deleted_subscription_is_gone_and_no_longer_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_d_type = namespace.entity_type("VehicleD")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_unknown_id = namespace.subscription_id("unknown")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_d_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id)
        .entities([vehicle_d_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .registration_subscriptions()
        .delete(&subscription_unknown_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker, Method::DELETE, "csourceSubscriptions/invalidUri")?;
    let answer_3 = broker.send_unchecked(request).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_d_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    sink.wait_for_at_least(2, 1).await?;
    let notifications = sink.received(2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(0)));
    Ok(())
}
