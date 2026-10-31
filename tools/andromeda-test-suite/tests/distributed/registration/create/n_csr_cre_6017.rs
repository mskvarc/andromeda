//! An auxiliary registration is limited to consumption operations.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Create Context Source Registration with mode auxiliary answers 400 `BadRequestData` for operations
/// `createEntity` and 201 for `retrieveEntity` with `queryEntity`
/// Clause: 5.9.2.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_csr_cre_6017_an_auxiliary_registration_is_limited_to_consumption_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `mode` is written as text.
    let mode: RawJson = r#""auxiliary""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("mode", &mode);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let registration_3 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_2_id).send().await?;
    expect_that!(registration_3.mode(), some(eq(RegistrationMode::Auxiliary)));
    expect_that!(
        registration_3.operations().and_then(|operations| operations.first()),
        some(eq(&"retrieveEntity".parse::<SupportedOperation>()?))
    );
    expect_that!(
        registration_3.operations().and_then(|operations| operations.get(1)),
        some(eq(&"queryEntity".parse::<SupportedOperation>()?))
    );
    Ok(())
}
