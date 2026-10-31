//! An unreachable context outranks an undecodable body.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    partial_outcome::reported,
    problem::problem,
    report_matcher::batch_error,
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
use ngsi_ld::{http::BatchOperationResult, model::ErrorType};

/// Pins: Create Entity with a numeric type, Query Entities, Query Temporal Evolution of Entities and Create
/// `EntityMap` via POST with an unknown entity selector member, and Batch Entity Delete with a number among the
/// ids, all under an unreachable @context, answer 504 `LdContextNotAvailable`; Batch Entity Creation reports
/// `LdContextNotAvailable` against the entity naming the unreachable @context and `BadRequestData` against the one
/// naming the Core @context
/// Clause: 5.5.4, 5.5.6, 6.3.2, 5.6.7.4
///
/// Invalid type: 5 (a number where the entity type belongs, refused once the @context loads)
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9026_an_unreachable_context_outranks_an_undecodable_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let core = config.core_context();
    let unreachable_link =
        format!(r#"<{unreachable}/ctx/{namespace}/unreachable.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#);
    // Every body is one the model cannot write (a numeric type, an unknown selector member, a number among ids), and
    // the point is that the context outranks the body, so each is sent as written.
    let create_body = r#"{
    "id": "urn:ngsi-ld:Device:{{ns}}-1",
    "type": 5
}"#
    .replace("{{ns}}", &namespace.to_string());
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(LINK, HeaderValue::from_str(&unreachable_link)?)
        .body(Bytes::from(create_body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let query_body = r#"{
    "type": "Query",
    "entities": [
        {
            "type": "Device_{{ns}}",
            "bogus": 1
        }
    ]
}"#
    .replace("{{ns}}", &namespace.to_string());
    for path in ["entityOperations/query", "temporal/entityOperations/query", "entityMaps"] {
        let request_2 = request_to(&broker, Method::POST, path)?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .append_header(LINK, HeaderValue::from_str(&unreachable_link)?)
            .body(Bytes::from(query_body.clone()));
        let answer_2 = broker.send_unchecked(request_2).await;
        expect_that!(&answer_2, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    }
    let delete_body = r#"[
    "urn:ngsi-ld:Device:{{ns}}-a",
    42
]"#
    .replace("{{ns}}", &namespace.to_string());
    let request_3 = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(LINK, HeaderValue::from_str(&unreachable_link)?)
        .body(Bytes::from(delete_body));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    // The model writes one @context into the whole batch body, so a batch whose items each carry their own is sent
    // as written.
    let batch_body = r#"[
    {
        "id": "urn:ngsi-ld:Device:{{ns}}-unreachable",
        "type": 5,
        "@context": [
            "{{unreachable}}/ctx/{{ns}}/unreachable.jsonld"
        ]
    },
    {
        "id": "urn:ngsi-ld:Device:{{ns}}-reachable",
        "type": 5,
        "@context": [
            "{{core}}"
        ]
    }
]"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{unreachable}}", &unreachable)
    .replace("{{core}}", core.as_str());
    let request_4 = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(batch_body));
    let answer_4 = broker.send_unchecked(request_4).await;
    let report: BatchOperationResult = reported(answer_4)?;
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&namespace.urn("Device", "unreachable")?)
                .problem(ErrorType::LdContextNotAvailable)
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&namespace.urn("Device", "reachable")?).problem(ErrorType::BadRequestData))
    );
    Ok(())
}
