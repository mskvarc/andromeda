//! A notified registration keeps only its matching registration info.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration on broker A with one `RegistrationInfo` of the subscribed type and one
/// of another type is notified to a subscription on broker A with the registration holding only the matching
/// `RegistrationInfo`
/// Clause: 5.11.7, 5.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_7608_a_notified_registration_keeps_only_its_matching_registration_info() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let other_type = namespace.entity_type("Other")?;
    let subscription = Subscription::registration_builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(namespace.registration_id(1)?)
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
                .build()?,
            RegistrationInfo::builder()
                .entities([EntityInfo::new(other_type).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
                .build()?,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.registrations()?;
    let notified = notification.data().first().or_fail()?;
    expect_that!(notified.information(), len(eq(1)));
    let entities = notified.information().first().or_fail()?.entities();
    expect_that!(entities.first().map(EntityInfo::entity_type), some(eq(&OneOrMany::from(vehicle_type))));
    Ok(())
}
