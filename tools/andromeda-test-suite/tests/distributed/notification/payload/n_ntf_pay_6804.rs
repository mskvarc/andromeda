//! A reassembled notification leaves out the entities the subscription q does not select.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
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
    query::q,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: With a split subscription whose q is speed>50, appending an attribute to two entities on peer B notifies
/// the subscriber of the one with speed 90 alone and never of the one with speed 10
/// Clause: 5.8.6, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6804_a_reassembled_notification_leaves_out_the_entities_the_subscription_q_does_not_select() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_hi_id = namespace.urn("Vehicle", "hi")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_lo_id = namespace.urn("Vehicle", "lo")?;
    let registration_b_id = namespace.registration_id("b")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_hi = Entity::builder()
        .id(vehicle_hi_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 90)
        .build()?;
    broker_b.entities().create(&vehicle_hi).send().await?.done()?;
    let vehicle_lo = Entity::builder()
        .id(vehicle_lo_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker_b.entities().create(&vehicle_lo).send().await?.done()?;
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
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .q(q::attr("speed").gt(50).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription splitEntities Split), so `splitEntities` is written as text.
    let split_entities: RawJson = "true".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("splitEntities", &split_entities);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("mileage", 1).build()?;
    broker_b.entities().append_attributes(&vehicle_lo_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("mileage", 2).build()?;
    broker_b.entities().append_attributes(&vehicle_hi_id, &fragment_2).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-hi')]").parse()?,
    )];
    let notifications = sink.wait_for_at_least_where(1, &selected, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_hi_id)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(1)));
    Ok(())
}
