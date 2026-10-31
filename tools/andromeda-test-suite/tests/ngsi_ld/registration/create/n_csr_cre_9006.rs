//! An exclusive registration overlaps an id beside a pattern only through that id.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Over a redirect registration whose entity info names id Vehicle-1 beside an `idPattern` matching every
/// Vehicle of the namespace, creating an exclusive registration of speed on Vehicle-1 is a Conflict and stores
/// nothing, while one on Vehicle-2 answers 201
/// Clause: 4.3.6.3, 5.9.2.4, 5.2.8, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_cre_9006_an_exclusive_registration_overlaps_an_id_beside_a_pattern_only_through_that_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_redirect_id = namespace.registration_id("redirect")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_exclusive_1_id = namespace.registration_id("exclusive-1")?;
    let registration_exclusive_2_id = namespace.registration_id("exclusive-2")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_redirect = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_redirect_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone())
                .with_id(vehicle_id.clone())
                .with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    broker.registrations().register(&registration_redirect).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_redirect_cleanup = Cleanup::of(&broker).registration(&registration_redirect_id);
    let registration_exclusive_1 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_exclusive_1_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration_exclusive_1).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_exclusive_1_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    // Clause 5.2.8: the id takes precedence over the idPattern beside it, so the
    // redirect registration covers Vehicle-1 only.
    let registration_exclusive_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_exclusive_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_exclusive_2).send().await?;
    let _registration_exclusive_2_cleanup = Cleanup::of(&broker).registration(&registration_exclusive_2_id);
    Ok(())
}
