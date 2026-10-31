//! An endpoint reads back as written.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    raw_answer::send_raw,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    model::UrlDraft,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: A registration whose endpoint is an authority-only URI, the same with a trailing slash, one with a path
/// segment and one with a three-segment path reads it back with the spelling it was written with
/// Clause: 5.2.2, Table 5.2.9-1, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_ret_9003_an_endpoint_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let origin = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoints = [
        origin.clone(),
        format!("{origin}/"),
        format!("{origin}/broker1"),
        format!("{origin}/ngsi-ld/v1"),
    ];
    for (index, endpoint) in endpoints.iter().enumerate() {
        let registration_id = namespace.registration_id(format!("spelling-{index}"))?;
        let vehicle_id = namespace.urn("Vehicle", format!("spelling-{index}"))?;
        let registration = CSourceRegistration::builder(UrlDraft::verbatim(endpoint))
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
                .property_names(["speed"])
                .build()?])
            .mode(RegistrationMode::Exclusive)
            .operations([DistributedOperation::RetrieveEntity])
            .build()?;
        let answer = broker.with_responses().registrations().register(&registration).send().await;
        expect_that!(&answer, answered(StatusCode::CREATED));
        // The endpoint spelling is the subject and the typed endpoint is the normalized URL, so the answer is read as
        // received.
        let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let response = send_raw(read).await?;
        expect_that!(response.status(), eq(StatusCode::OK));
        expect_that!(response.body(), json_equals("$.endpoint", endpoint.clone()));
        broker.registrations().delete(&registration_id).send().await?;
    }
    Ok(())
}
