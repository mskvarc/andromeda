//! An auxiliary registration cannot be updated to a write operation.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{
        CSourceRegistration,
        DistributedOperation,
        EntityInfo,
        RegistrationFragment,
        RegistrationInfo,
        RegistrationMode,
        RegistrationTypeMember,
        SupportedOperation,
    },
};

/// Pins: Update Context Source Registration of an auxiliary registration with operations `updateEntity` answers 400
/// `BadRequestData`, and the operations read back unchanged
/// Clause: 5.9.3.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_csr_upd_6007_an_auxiliary_registration_cannot_be_updated_to_a_write_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder()
        .operations([DistributedOperation::UpdateEntity])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        registration_2.operations().and_then(|operations| operations.first()),
        some(eq(&"retrieveEntity".parse::<SupportedOperation>()?))
    );
    expect_that!(
        registration_2.operations().and_then(|operations| operations.get(1)),
        some(eq(&"queryEntity".parse::<SupportedOperation>()?))
    );
    expect_that!(registration_2.operations().map(<[_]>::len), some(eq(2)));
    Ok(())
}
