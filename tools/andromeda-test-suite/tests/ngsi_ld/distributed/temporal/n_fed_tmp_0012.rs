//! A temporal retrieve takes the exclusive attribute from the source and keeps the local one.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Temporal Evolution of an Entity holding `fuelLevel` locally whose speed is registered exclusively
/// answers the source speed instance of value 42 and the local `fuelLevel`
/// Clause: 5.7.3.4, 4.3.6.2, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0012_a_temporal_retrieve_takes_the_exclusive_attribute_from_the_source_and_keeps_the_local_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuelLevel", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(42).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).temporal().retrieve(&vehicle_id).send().await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", contains(any_attribute().value(42))));
    expect_that!(evolution, has_evolution("fuelLevel", nth(0, any_attribute().value(60))));
    Ok(())
}
