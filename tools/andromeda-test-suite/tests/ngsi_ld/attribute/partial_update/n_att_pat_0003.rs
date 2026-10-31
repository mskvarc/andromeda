//! A patch without an entity id is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Partial Attribute Update addressed to an empty Entity id path segment is refused with 400 `BadRequestData`
/// Clause: 5.6.4.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_att_pat_0003_a_patch_without_an_entity_id_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, "entities//attrs/speed")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"value\": 100\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
