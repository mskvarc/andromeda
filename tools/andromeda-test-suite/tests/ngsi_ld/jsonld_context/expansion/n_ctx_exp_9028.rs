//! A link header beside an ld json body is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Link naming the version 1.9 core context, as the request writes it.
const CORE_LINK: &str =
    r#"<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

/// Pins: Batch create, upsert and update and Create Entity of an application/ld+json body sent with a JSON-LD Link
/// header answer 400 `BadRequestData`
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9028_a_link_header_beside_an_ld_json_body_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "context-mix")?;
    let batch_body = r#"[
    {
        "id": "urn:ngsi-ld:Vehicle:{{ns}}-context-mix",
        "type": "Vehicle_{{ns}}",
        "@context": [
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
        ]
    }
]"#
    .replace("{{ns}}", &namespace.to_string());
    let body = r#"{
    "id": "urn:ngsi-ld:Vehicle:{{ns}}-context-mix",
    "type": "Vehicle_{{ns}}",
    "@context": [
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
}"#
    .replace("{{ns}}", &namespace.to_string());
    // A Link beside the body context is the pair the broker refuses, and no typed request carries both.
    for (path, text) in [
        ("entityOperations/create", &batch_body),
        ("entityOperations/upsert", &batch_body),
        ("entityOperations/update", &batch_body),
        ("entities", &body),
    ] {
        let request = request_to(&broker, Method::POST, path)?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
            .append_header(LINK, HeaderValue::from_static(CORE_LINK))
            .body(Bytes::from(text.clone()));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    Ok(())
}
