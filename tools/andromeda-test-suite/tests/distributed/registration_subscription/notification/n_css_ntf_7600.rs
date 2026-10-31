//! A new registration is notified with the full structure.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration on broker A matching a subscription on broker A is notified as a
/// `ContextSourceNotification` with a URI id, the subscription id, `notifiedAt`, `triggerReason` `newlyMatching`
/// and the registration in data
/// Clause: 5.11.7, 5.3.2
#[gtest]
#[tokio::test]
async fn n_css_ntf_7600_a_new_registration_is_notified_with_the_full_structure() -> Result<()> {
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
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.id().to_string(), contains_regex("^[A-Za-z][A-Za-z0-9+.-]*:.+"));
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.notified_at().to_string(), contains_regex("^20[0-9][0-9]-"));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    Ok(())
}
