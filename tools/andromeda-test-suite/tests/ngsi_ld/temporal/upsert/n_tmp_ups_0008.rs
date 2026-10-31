//! A missing type is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: a Temporal Evolution without a type member is refused with 400 `BadRequestData` and the entity is not created
/// Clause: 5.6.11.4, 5.5.4, 5.2.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0008_a_missing_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    // The model requires the `type` this Temporal Evolution leaves out, so it is sent as written.
    let body = format!(r#"{{"id": "{vehicle_id}", "speed": [{{"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"}}]}}"#);
    let request = json_request_to(&broker, Method::POST, "temporal/entities", &body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&missing, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
