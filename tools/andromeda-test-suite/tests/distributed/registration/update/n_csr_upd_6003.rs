//! An invalid json ld context is bad request data.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with an application/ld+json fragment whose @context is the number 5
/// answers 400 `BadRequestData`, and the description reads back unchanged
/// Clause: 5.9.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_csr_upd_6003_an_invalid_json_ld_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .description("original")
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request = request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:{namespace}-1"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
    .body(Bytes::from("{\n    \"description\": \"changed\",\n    \"@context\": 5\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.description(), some(eq("original")));
    Ok(())
}
