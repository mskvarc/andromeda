//! A projection is decided after the merge.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity with attrs naming an attribute only broker B holds answers it with 200 after the merge,
/// and with attrs naming an attribute no broker holds is answered 404 `ResourceNotFound`
/// Clause: 5.7.1.4, 4.5.5.1
#[gtest]
#[tokio::test]
async fn n_fed_ret_7408_a_projection_is_decided_after_the_merge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuelLevel", 70)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["fuelLevel"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["fuelLevel".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().value(70)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["color".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
