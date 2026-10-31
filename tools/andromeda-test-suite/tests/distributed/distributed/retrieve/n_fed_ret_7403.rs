//! A source attribute expired in the past is discarded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity whose inclusive source answers speed with an `expiresAt` in the past and `fuelLevel` with
/// an `expiresAt` in the future answers `fuelLevel` and drops the expired speed
/// Clause: 5.7.1.4, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_fed_ret_7403_a_source_attribute_expired_in_the_past_is_discarded() -> Result<()> {
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
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(90).expires_at("2000-01-01T00:00:00Z".parse()?))
        .property("fuelLevel", Property::builder(70).expires_at("2999-01-01T00:00:00Z".parse()?))
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().value(70)));
    Ok(())
}
