//! A creation without a temporal query is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with a type and no temporal query answers 400
/// `BadRequestData`
/// Clause: 5.14.5.4, 4.11
#[gtest]
#[tokio::test]
async fn n_map_qte_7000_a_creation_without_a_temporal_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client refuses a temporal map without a temporal query before sending it, so the request is sent as written.
    let vehicle_type = namespace.suffixed("Vehicle");
    let request = request_to(&broker, Method::GET, &target("temporal/entityMaps", &[("type", vehicle_type.as_str())]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
