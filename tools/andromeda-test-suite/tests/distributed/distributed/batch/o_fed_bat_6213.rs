//! Upsert of an existing entity without a follow up operation is operation not supported.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert under a redirect registration advertising `createEntity` alone, whose source answers
/// the create with 409 `AlreadyExists`, answers 207 with an error of status 422 for the entity naming the
/// registration
/// Clause: 5.6.8.4, 5.2.17
/// Recorded: 5.2.17 and 5.2.19 make `registrationId` optional (0..1) in an error entry; Andromeda always sets it
/// and the file pins that.
#[gtest]
#[tokio::test]
async fn o_fed_bat_6213_upsert_of_an_existing_entity_without_a_follow_up_operation_is_operation_not_supported() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
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
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CONFLICT))
        .await?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?];
    let report = broker.entities().batch_upsert(&entities).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&vehicle_id)
                .registration(&registration_id)
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .problem(ErrorType::OperationNotSupported)
        )
    );
    Ok(())
}
