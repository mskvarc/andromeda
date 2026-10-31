//! An update notifies every matching registration again.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription on broker A of a subscription matching two registrations
/// answers 204 and is followed by a second notification listing both registrations
/// Clause: 5.11.3.4, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_upd_7602_an_update_notifies_every_matching_registration_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    for (id, tail) in [(&registration_id, "1-.*"), (&registration_2_id, "2-.*")] {
        let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
            .id(id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", tail)?)])
                .build()?])
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let fragment = SubscriptionFragment::registration_builder().description("re-evaluated").build()?;
    broker.registration_subscriptions().update(&subscription_id, &fragment).send().await?;
    let notifications = sink.wait_for_at_least(1, 2).await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(2)));
    let ids: Vec<&_> = notification.data().iter().map(CSourceRegistration::<Output>::id).collect();
    expect_that!(ids.contains(&&registration_id), eq(true));
    expect_that!(ids.contains(&&registration_2_id), eq(true));
    Ok(())
}
