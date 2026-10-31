//! Two failing query sources add a warning each.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    context_source::ContextSource,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type whose two matching registrations name a source answering 500 and an origin where
/// nothing listens answers 200 with two NGSILD-Warning values, codes 299 and 199
/// Clause: 6.3.17, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_err_0006_two_failing_query_sources_add_a_warning_each() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.warnings().valid().map(|warnings| warnings.iter().count()), some(eq(2)));
    Ok(())
}
