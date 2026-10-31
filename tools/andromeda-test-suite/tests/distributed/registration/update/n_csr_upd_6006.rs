//! An update making an exclusive or redirect registration overlap is a conflict.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationMode, RegistrationTypeMember},
};

/// Pins: Update Context Source Registration so an exclusive registration covers an entity id and attribute another
/// exclusive registration or an existing entity already holds, or so a redirect registration matches an existing
/// entity, answers 409 Conflict and leaves the registration as it was
/// Clause: 5.9.3.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_csr_upd_6006_an_update_making_an_exclusive_or_redirect_registration_overlap_is_a_conflict() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
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
            .property_names(["fuel"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["fuel", "speed"])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_2_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let registration_3 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_2_id).send().await?;
    expect_that!(registration_3.information().first().map(|info| info.property_names().len()), some(eq(1)));
    expect_that!(
        registration_3
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("fuel"))
    );
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let registration_3_2 = CSourceRegistration::builder(source.endpoint("3")?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["fuel"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_3_2).send().await?;
    let _registration_3_2_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let fragment_2 = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["fuel", "speed"])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer_2 = broker.with_responses().registrations().update(&registration_3_id, &fragment_2).send().await;
    expect_that!(&answer_2, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let registration_4 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_3_id).send().await?;
    expect_that!(registration_4.information().first().map(|info| info.property_names().len()), some(eq(1)));
    expect_that!(
        registration_4
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("fuel"))
    );
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle_3).send().await?.done()?;
    let registration_4_2 = CSourceRegistration::builder(source.endpoint("4")?)
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_4_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    broker.registrations().register(&registration_4_2).send().await?;
    let _registration_4_2_cleanup = Cleanup::of(&broker).registration(&registration_4_id);
    let fragment_3 = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer_3 = broker.with_responses().registrations().update(&registration_4_id, &fragment_3).send().await;
    expect_that!(&answer_3, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let registration_5 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_4_id).send().await?;
    expect_that!(
        registration_5
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        some(eq(&vehicle_4_id))
    );
    Ok(())
}
