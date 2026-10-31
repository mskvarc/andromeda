//! An empty array is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: a Batch Entity Upsert whose body is an empty array is refused as `BadRequestData`
/// Clause: 5.6.8.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0014_an_empty_array_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client refuses an empty batch before sending it (clause 5.6.10), so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/upsert")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("[]"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
