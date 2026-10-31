//! An empty id array is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: a Batch Entity Delete sent as application/json with an empty id array is refused as `BadRequestData`
/// Clause: 5.6.10.4
#[gtest]
#[tokio::test]
async fn n_bat_del_0004_an_empty_id_array_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client refuses an empty batch before sending it (clause 5.6.10), so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("[]"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
