//! A context nested in information or location is bad request data.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    context_source::ContextSource,
    namespace::Namespace,
    problem::problem,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{APPLICATION_LD_JSON, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration refuses a user @context nested in an information member and in
/// location, and leaves the registration unchanged
/// Clause: 5.5.7, 5.9.3
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_csr_upd_2001_a_context_nested_in_information_or_location_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .description("before")
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id(device_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let core = SuiteConfig::get()?.core_context();
    let path = format!("csourceRegistrations/{registration_id}");
    // Each fragment nests a user `@context` the model refuses, so the bodies are written as text.
    let nested_information = format!(
        r#"{{"description": "after", "information": [{{"entities": [{{"type": "{device_type}", "id": "{device_id}"}}], "@context": {{}}}}], "@context": "{core}"}}"#
    );
    let request = content_type(request_to(&broker, Method::PATCH, &path)?, &APPLICATION_LD_JSON)?.body(Bytes::from(nested_information));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let nested_location =
        format!(r#"{{"description": "after", "location": {{"type": "Point", "coordinates": [13.4, 52.5], "@context": {{}}}}, "@context": "{core}"}}"#);
    let request_2 = content_type(request_to(&broker, Method::PATCH, &path)?, &APPLICATION_LD_JSON)?.body(Bytes::from(nested_location));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.description(), some(eq("before")));
    Ok(())
}
