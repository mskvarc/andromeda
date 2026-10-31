//! Inclusive create forwards the full input and creates locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity under an inclusive registration answers 201, forwards one create carrying `brandName` and
/// speed, and the Entity is created locally with both
/// Clause: 5.6.1.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6205_inclusive_create_forwards_the_full_input_and_creates_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::entity(forwarded_one)?;
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(42)));
    expect_that!(&forwarded_entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, has_attribute("speed", any_attribute().value(42)));
    Ok(())
}
