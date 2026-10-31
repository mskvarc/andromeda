//! Create forwards nothing to a source registering no attribute of the batch.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation of a Vehicle with speed alone, under redirect registrations of colour and of speed,
/// forwards one batch to the speed source and none to the colour source
/// Clause: 5.6.7.4, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_bat_6203_create_forwards_nothing_to_a_source_registering_no_attribute_of_the_batch() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["colour"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(0)));
    let forwarded_2 = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
