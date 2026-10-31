//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Partial Attribute Update addressed to an Entity the broker does not hold is refused with 404
/// `ResourceNotFound`
/// Clause: 5.6.4.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_att_pat_0007_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_unknown_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"value\": 100\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
