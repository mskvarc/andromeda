//! A notification from one holder of a split entity is completed from the other holders.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: With speed on peer B and colour on peer C for one entity and a split subscription on this broker,
/// appending mileage on B notifies the subscriber once, under the local subscription id, with speed, colour and
/// mileage
/// Clause: 5.8.6, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6803_a_notification_from_one_holder_of_a_split_entity_is_completed_from_the_other_holders() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("colour", "red")
        .build()?;
    broker_c.entities().create(&vehicle_2).send().await?.done()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_b_cleanup = Cleanup::of(&broker).registration(&registration_b_id);
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_c_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    let _registration_c_cleanup = Cleanup::of(&broker).registration(&registration_c_id);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription splitEntities Split), so `splitEntities` is written as text.
    let split_entities: RawJson = "true".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("splitEntities", &split_entities);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("mileage", 1000).build()?;
    broker_b.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&vehicle_id),
                has_attribute("speed", any_attribute().value(40)),
                has_attribute("colour", any_attribute().value("red")),
                has_attribute("mileage", any_attribute().value(1000))
            )
        )
    );
    Ok(())
}
