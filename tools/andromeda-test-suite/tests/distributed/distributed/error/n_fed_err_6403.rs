//! A source declining the requested representation adds warning 299.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RepresentationSpelling,
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with options `keyValues` whose only matching source answers 501 answers 200 with no
/// Entity and NGSILD-Warning code 299
/// Clause: 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_err_6403_a_source_declining_the_requested_representation_adds_warning_299() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::status(StatusCode::NOT_IMPLEMENTED))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(
        page.warnings().valid().and_then(|warnings| warnings.iter().next()).map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    expect_that!(page.items(), is_empty());
    Ok(())
}
