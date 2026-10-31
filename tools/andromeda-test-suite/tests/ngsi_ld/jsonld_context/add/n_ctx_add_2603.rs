//! A string json document is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add @context with a body that is valid JSON but a string is refused as `BadRequestData`, not
/// `InvalidRequest`
/// Clause: 5.13.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_add_2603_a_string_json_document_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The body is no document holding only an @context, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!("\"https://test.andromeda/{namespace}/context.jsonld\"")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
