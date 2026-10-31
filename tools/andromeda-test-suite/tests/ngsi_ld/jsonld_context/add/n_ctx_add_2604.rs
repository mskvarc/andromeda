//! A body with other members but no context is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add @context with a JSON object whose only members are other than @context (a nested object, a number, a
/// boolean) is refused as `BadRequestData`
/// Clause: 5.13.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_add_2604_a_body_with_other_members_but_no_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The body is no document holding only an @context, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"context\": {{ \"Building_{namespace}\": \"https://test.andromeda/{namespace}/Building\" }}\n}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The body is no document holding only an @context, which the model cannot write, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"@vocab\": \"https://test.andromeda/{namespace}/\",\n    \"n\": 1,\n    \"b\": true\n}}"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!("{{\n    \"A1\": \"https://test.andromeda/{namespace}/a1\"\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
