//! An empty entity id is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Add Attributes addressed to an empty Entity id path segment is refused with 400 `BadRequestData`, with a
/// title
/// Clause: 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_0007_an_empty_entity_id_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, "temporal/entities//attrs")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"added_attribute\": [\n        { \"type\": \"Property\", \"value\": 120, \"observedAt\": \"2020-09-01T12:03:00Z\" },\n        { \"type\": \"Property\", \"value\": 80, \"observedAt\": \"2020-09-01T12:05:00Z\" },\n        { \"type\": \"Property\", \"value\": 100, \"observedAt\": \"2020-09-01T12:07:00Z\" }\n    ]\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
