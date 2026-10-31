//! An update of an auxiliary registration to write operations is bad request data and changes nothing.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Update Context Source Registration of an auxiliary registration with the operations `createEntity` answers
/// 400 `BadRequestData` and the registration keeps `retrieveEntity`
/// Clause: 5.9.3.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_csr_upd_7200_an_update_of_an_auxiliary_registration_to_write_operations_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder().operations([DistributedOperation::CreateEntity]).build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        registration_2.operations().and_then(|operations| operations.first()),
        some(eq(&"retrieveEntity".parse::<SupportedOperation>()?))
    );
    expect_that!(registration_2.operations().map(<[_]>::len), some(eq(1)));
    Ok(())
}
