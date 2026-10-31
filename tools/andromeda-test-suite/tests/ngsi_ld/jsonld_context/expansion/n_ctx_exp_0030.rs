//! An ld json body refuses a context link and admits a profile link.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
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
use ngsi_ld::model::ErrorType;

/// Pins: Create Entity and Batch Entity Creation as application/ld+json beside a context Link are refused as
/// `BadRequestData` and create nothing, and Create Entity as application/ld+json beside a profile Link alone
/// answers 201
/// Clause: 6.3.5, 6.3.21
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0030_an_ld_json_body_refuses_a_context_link_and_admits_a_profile_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let vehicle_id = namespace.urn("Vehicle", "1")?;
    let body = r#"{
    "id": "urn:ngsi-ld:Vehicle:{{ns}}-1",
    "type": "Vehicle_{{ns}}",
    "@context": "{{core}}"
}"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{core}}", core.as_str());
    // No typed request carries a Link header beside an application/ld+json body context: the pair is what the test
    // refuses.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#))?,
        )
        .body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing = broker.with_responses().entities().retrieve(&vehicle_id).send().await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    let vehicle_2_id = namespace.urn("Vehicle", "2")?;
    let batch_body = r#"[
    {
        "id": "urn:ngsi-ld:Vehicle:{{ns}}-2",
        "type": "Vehicle_{{ns}}",
        "@context": "{{core}}"
    }
]"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{core}}", core.as_str());
    let request_2 = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#))?,
        )
        .body(Bytes::from(batch_body));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing_2 = broker.with_responses().entities().retrieve(&vehicle_2_id).send().await;
    expect_that!(&missing_2, answered(StatusCode::NOT_FOUND));
    let profile_body = r#"{
    "id": "urn:ngsi-ld:Vehicle:{{ns}}-3",
    "type": "Vehicle_{{ns}}",
    "@context": "{{core}}"
}"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{core}}", core.as_str());
    // The profile Link is no NGSI-LD context, and no typed request carries one beside a body context.
    let request_3 = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(LINK, HeaderValue::from_static(r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#))
        .body(Bytes::from(profile_body));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    Ok(())
}
