//! An id that is not a uri is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add Attributes addressed to an Entity id that is not a URI is refused with 400 `BadRequestData`, with a
/// title
/// Clause: 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_0008_an_id_that_is_not_a_uri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/invalid-{namespace}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"added_attribute\": [\n        { \"type\": \"Property\", \"value\": 120, \"observedAt\": \"2020-09-01T12:03:00Z\" },\n        { \"type\": \"Property\", \"value\": 80, \"observedAt\": \"2020-09-01T12:05:00Z\" },\n        { \"type\": \"Property\", \"value\": 100, \"observedAt\": \"2020-09-01T12:07:00Z\" }\n    ]\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
