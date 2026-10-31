//! An exclusive registration over a claimed footprint is a conflict.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Context Source Registration with mode exclusive for an entity id and attribute an exclusive or
/// redirect registration already matches, or an existing entity already holds, answers 409 Conflict and creates
/// nothing, while an exclusive registration for an attribute the existing entity lacks answers 201
/// Clause: 5.9.2.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_csr_cre_6015_an_exclusive_registration_over_a_claimed_footprint_is_a_conflict() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_4_id = namespace.registration_id(4)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_5_id = namespace.registration_id(5)?;
    let registration_6_id = namespace.registration_id(6)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration_2).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let registration_3 = CSourceRegistration::builder(source.endpoint("3")?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let registration_4 = CSourceRegistration::builder(source.endpoint("4")?)
        .id(registration_4_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    let answer_3 = broker.with_responses().registrations().register(&registration_4).send().await;
    expect_that!(&answer_3, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle_3).send().await?.done()?;
    let registration_5 = CSourceRegistration::builder(source.endpoint("5")?)
        .id(registration_5_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    let answer_4 = broker.with_responses().registrations().register(&registration_5).send().await;
    expect_that!(&answer_4, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_5_id)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let registration_6 = CSourceRegistration::builder(source.endpoint("6")?)
        .id(registration_6_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .property_names(["fuel"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_6).send().await?;
    let _registration_6_cleanup = Cleanup::of(&broker).registration(&registration_6_id);
    Ok(())
}
