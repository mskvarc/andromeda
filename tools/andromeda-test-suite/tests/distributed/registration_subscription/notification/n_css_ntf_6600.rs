//! Temporal gating notifies only the registrations with the relevant interval.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{NgsiLdMediaType, Output, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription without `temporalQ` is notified of a registration without time
/// intervals and not of an `observationInterval` one, and one with `temporalQ` of the registrations whose relevant
/// interval the query reaches (`observedAt` after, `createdAt` on a `managementInterval`, before, between) and not
/// of those lacking it
/// Clause: 5.11.1, 5.11.7, 5.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_6600_temporal_gating_notifies_only_the_registrations_with_the_relevant_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_a_type = namespace.entity_type("VehicleA")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_b_type = namespace.entity_type("VehicleB")?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let vehicle_e_type = namespace.entity_type("VehicleE")?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let vehicle_f_type = namespace.entity_type("VehicleF")?;
    let subscription_5_id = namespace.subscription_id(5)?;
    let vehicle_g_type = namespace.entity_type("VehicleG")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let registration_5_id = namespace.registration_id(5)?;
    let registration_6_id = namespace.registration_id(6)?;
    let registration_7_id = namespace.registration_id(7)?;
    let registration_8_id = namespace.registration_id(8)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_a_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id)
        .entities([vehicle_b_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "observedAt"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_2.render())?.set_member("temporalQ", &temporal_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_3 = Subscription::registration_builder()
        .id(subscription_3_id)
        .entities([vehicle_e_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_2: RawJson = r#"{"timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "createdAt"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_3.render())?.set_member("temporalQ", &temporal_q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_3 })?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let subscription_4 = Subscription::registration_builder()
        .id(subscription_4_id)
        .entities([vehicle_f_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_3: RawJson = r#"{"timerel": "before", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "observedAt"}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_4.render())?.set_member("temporalQ", &temporal_q_3);
    let request_3 = broker.unchecked(&CreateSubscription { subscription: &subscription_4 })?.document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let subscription_5 = Subscription::registration_builder()
        .id(subscription_5_id)
        .entities([vehicle_g_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(5)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_4: RawJson =
        r#"{"timerel": "between", "timeAt": "2020-06-01T00:00:00Z", "endTimeAt": "2020-09-01T00:00:00Z", "timeproperty": "observedAt"}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_5.render())?.set_member("temporalQ", &temporal_q_4);
    let request_4 = broker.unchecked(&CreateSubscription { subscription: &subscription_5 })?.document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::CREATED));
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_a_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_a_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_2_id)));
    let registration_3 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_b_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let registration_4 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_b_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&broker).registration(&registration_4_id);
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.registrations()?;
    expect_that!(notification_2.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(
        notification_2.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_4_id))
    );
    let registration_5 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_5_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_e_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_5).send().await?;
    let _registration_5_cleanup = Cleanup::of(&broker).registration(&registration_5_id);
    let registration_6 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_6_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_e_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .management_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_6).send().await?;
    let _registration_6_cleanup = Cleanup::of(&broker).registration(&registration_6_id);
    sink.wait_for_at_least(3, 1).await?;
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.registrations()?;
    expect_that!(notification_3.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_3.data(), len(eq(1)));
    expect_that!(
        notification_3.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_6_id))
    );
    let registration_7 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_7_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_f_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_7).send().await?;
    let _registration_7_cleanup = Cleanup::of(&broker).registration(&registration_7_id);
    sink.wait_for_at_least(4, 1).await?;
    let notifications_4 = sink.received(4).await?;
    expect_that!(notifications_4, len(eq(1)));
    let notification_4 = notifications_4.first().or_fail()?.registrations()?;
    expect_that!(notification_4.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_4.data(), len(eq(1)));
    expect_that!(
        notification_4.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_7_id))
    );
    let registration_8 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_8_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_g_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_8).send().await?;
    let _registration_8_cleanup = Cleanup::of(&broker).registration(&registration_8_id);
    sink.wait_for_at_least(5, 1).await?;
    let notifications_5 = sink.received(5).await?;
    expect_that!(notifications_5, len(eq(1)));
    let notification_5 = notifications_5.first().or_fail()?.registrations()?;
    expect_that!(notification_5.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification_5.data(), len(eq(1)));
    expect_that!(
        notification_5.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_8_id))
    );
    Ok(())
}
