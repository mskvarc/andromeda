//! Create without either operation is a conflict per entity.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation under a redirect registration of speed advertising neither `createBatch` nor
/// `createEntity` answers 207 with a keyed 409 for each entity and forwards nothing
/// Clause: 5.6.7.4, 5.2.17
/// Recorded: 5.2.17 and 5.2.19 make `registrationId` optional (0..1) in an error entry; Andromeda always sets it
/// and the file pins that.
#[gtest]
#[tokio::test]
async fn o_fed_bat_6206_create_without_either_operation_is_a_conflict_per_entity() -> Result<()> {
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
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
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
    let report = broker.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().registration(&registration_id).status(StatusCode::CONFLICT))
    );
    expect_that!(
        &report.errors,
        nth(1, batch_error().registration(&registration_id).status(StatusCode::CONFLICT))
    );
    let forwarded_request = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
