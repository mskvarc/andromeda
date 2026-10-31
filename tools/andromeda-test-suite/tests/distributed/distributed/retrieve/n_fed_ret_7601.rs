//! Exclusive redirect and inclusive registrations each serve the attribute from the peer.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A of three Entities held only by broker B or broker C, each under an exclusive,
/// a redirect or an inclusive registration of speed with `retrieveEntity`, answers 200 with the peer's speed for
/// each, while broker A holds none of them locally
/// Clause: 5.7.1.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_ret_7601_exclusive_redirect_and_inclusive_registrations_each_serve_the_attribute_from_the_peer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 51)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 52)
        .build()?;
    broker_c.entities().create(&vehicle_2).send().await?.done()?;
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 53)
        .build()?;
    broker_b.entities().create(&vehicle_3).send().await?.done()?;
    let registration_3 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(51)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_2_id).send().await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(52)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let retrieved_3 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_3_id).send().await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_attribute("speed", any_attribute().value(53)));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_3_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
