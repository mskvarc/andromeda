//! A source answering malformed json adds warning 111.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type whose only matching source answers 200 with application/json text that is not valid
/// JSON answers 200 with no Entity and NGSILD-Warning code 111
/// Clause: 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_err_6402_a_source_answering_malformed_json_adds_warning_111() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    source
        .answer(
            Method::GET,
            "/ngsi-ld/v1/entities",
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), "{ this is not valid json"),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(
        page.warnings().valid().and_then(|warnings| warnings.iter().next()).map(NgsiLdWarning::code),
        some(eq(WarningCode::RevalidationFailed))
    );
    expect_that!(page.items(), is_empty());
    Ok(())
}
