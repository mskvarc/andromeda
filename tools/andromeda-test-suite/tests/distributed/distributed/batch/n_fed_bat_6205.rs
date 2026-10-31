//! Create falls back to one create per entity when the batch operation is not advertised.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation under a redirect registration advertising `createEntity` but not `createBatch`
/// answers 201 and forwards one create per entity
/// Clause: 5.6.7.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_bat_6205_create_falls_back_to_one_create_per_entity_when_the_batch_operation_is_not_advertised() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id)
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id)
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(2)));
    let forwarded_2 = source.received_on(Method::POST, "/a/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
