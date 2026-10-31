//! An array context holding a number is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add @context with an @context array that holds a number beside an object is refused as `BadRequestData`
/// and stores nothing
/// Clause: 5.13.2.3, 5.13.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_add_2602_an_array_context_holding_a_number_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The body holds an @context member the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"@context\": [\n        {{ \"Building_{namespace}\": \"https://test.andromeda/{namespace}/Building\" }},\n        5\n    ]\n}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    Ok(())
}
