//! A boolean context member is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add @context with an @context member that is a boolean is refused as `BadRequestData` and stores nothing
/// Clause: 5.13.2.3, 5.13.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_add_2601_a_boolean_context_member_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The body holds an @context member the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"@context\": true\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    Ok(())
}
