//! A missing id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::temporal_query,
};

/// Pins: a Temporal Evolution without an id member is refused with 400 `BadRequestData` and nothing of it is stored
/// Clause: 5.6.11.4, 5.5.4, 5.2.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0007_a_missing_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // The model requires the `id` this Temporal Evolution leaves out, so it is sent as written.
    let body = format!(r#"{{"type": "{vehicle_type}", "speed": [{{"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"}}]}}"#);
    let request = json_request_to(&broker, Method::POST, "temporal/entities", &body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
