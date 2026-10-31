//! A deletion is notified as no longer matching.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, cleanup::Cleanup, config::PeerRole, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Context Source Registration on broker A of a registration matching a subscription is notified with
/// `triggerReason` `noLongerMatching`
/// Clause: 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_ntf_7604_a_deletion_is_notified_as_no_longer_matching() -> Result<()> {
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
    let selected = [BodyPattern::Selects("$[?(@.triggerReason == 'newlyMatching')]".parse()?)];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    broker.registrations().delete(&registration_id).send().await?;
    let selected_2 = [BodyPattern::Selects("$[?(@.triggerReason == 'noLongerMatching')]".parse()?)];
    sink.wait_for_at_least_where(1, &selected_2, 1).await?;
    let selected_3 = [BodyPattern::Selects("$[?(@.triggerReason == 'noLongerMatching')]".parse()?)];
    let notifications = sink.received_where(1, &selected_3).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    Ok(())
}
