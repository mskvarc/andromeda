//! A context in the wrong body or none in the right one is refused.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Create Entity as application/json with an @context member naming the core, and as application/ld+json with
/// no @context member, are each refused as `BadRequestData`
/// Clause: 6.3.5, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0031_a_context_in_the_wrong_body_or_none_in_the_right_one_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    // The client writes no `@context` into an application/json body, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-1\",\n    \"type\": \"Vehicle_{namespace}\",\n    \"@context\": \"{core}\"\n}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-2\",\n    \"type\": \"Vehicle_{namespace}\"\n}}"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
