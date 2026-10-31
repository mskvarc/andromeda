//! A body that is not valid json is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Append Attributes whose payload is not a valid JSON document is refused with 400 `InvalidRequest`, with a
/// title
/// Clause: 5.5.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_att_app_0007_a_body_that_is_not_valid_json_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The body is not JSON, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, &format!("entities/{vehicle_id}/attrs"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from("{invalid json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}
