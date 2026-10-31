//! A context source info entry that is not an http header is bad request data.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Context Source Registration with a `contextSourceInfo` key with a space or a value with a line break
/// answers 400 `BadRequestData` and stores nothing, while a custom X-Api-Key pair answers 201
/// Clause: 6.3.19, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_7612_a_context_source_info_entry_that_is_not_an_http_header_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // Each pair breaks a rule of an HTTP header the model enforces, so the create documents are written as text.
    for (suffix, pair) in [(1, r#"{"key": "Bad Name", "value": "x"}"#), (2, r#"{"key": "X-Api-Key", "value": "a\nb"}"#)] {
        let id = namespace.registration_id(suffix)?;
        let body = format!(
            r#"{{"id": "{id}", "type": "ContextSourceRegistration", "information": [{{"entities": [{{"type": "{vehicle_type}", "idPattern": "urn:ngsi-ld:Vehicle:{namespace}-.*"}}]}}], "endpoint": "{endpoint}", "mode": "inclusive", "contextSourceInfo": [{pair}]}}"#,
            endpoint = source.endpoint("1")?
        );
        let answer = broker
            .send_unchecked(json_request_to(&broker, Method::POST, "csourceRegistrations", &body)?)
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let read = broker.with_responses().registrations().retrieve(&id).send().await;
        expect_that!(&read, answered(StatusCode::NOT_FOUND));
    }
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(namespace.registration_id(5)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .context_source_info(("X-Api-Key", "secret"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    Ok(())
}
