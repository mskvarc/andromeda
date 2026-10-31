//! Upsert merges the batch result of a source keyed by its registration.

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
    http::{BatchEntityError, BatchOperationResult},
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert under a redirect registration advertising `upsertBatch` whose source answers 207 with
/// one success and one `AlreadyExists` error answers 207 with that error naming the entity and the registration
/// Clause: 5.6.8.4, 5.2.16, 5.2.17
/// Recorded: 5.2.17 and 5.2.19 make `registrationId` optional (0..1) in an error entry; Andromeda always sets it
/// and the file pins that.
#[gtest]
#[tokio::test]
async fn o_fed_bat_6209_upsert_merges_the_batch_result_of_a_source_keyed_by_its_registration() -> Result<()> {
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
        .operations([DistributedOperation::UpsertBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_result = BatchOperationResult {
        success: vec![vehicle_id.clone()],
        errors: vec![BatchEntityError {
            entity_id: vehicle_2_id.clone(),
            error: source_answer::problem_without_detail(ErrorType::AlreadyExists, "AlreadyExists", Some(409)),
            registration_id: None,
        }],
    };
    source
        .answer(
            Method::POST,
            "/a/ngsi-ld/v1/entityOperations/upsert",
            source_answer::batch_result(&source_result),
        )
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    let report = broker.entities().batch_upsert(&entities).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.success, contains(eq(&vehicle_id)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error().entity(&vehicle_2_id).registration(&registration_id).status(StatusCode::CONFLICT)
        )
    );
    Ok(())
}
