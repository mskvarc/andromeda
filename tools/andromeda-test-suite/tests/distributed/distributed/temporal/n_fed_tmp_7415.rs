//! A query without a temporal query or a narrowing input is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Temporal Evolution of Entities without a temporal query, and with a temporal query but none of type,
/// attrs, q, `geoQ` or local, are each refused 400 `BadRequestData`
/// Clause: 5.7.4.4, 4.11
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7415_a_query_without_a_temporal_query_or_a_narrowing_input_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The typed query starts from its temporal query, so a query without one is sent as written.
    let request = request_to(&broker, Method::GET, &format!("temporal/entities?type=Vehicle_{namespace}"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client refuses a temporal query with no narrowing input before sending it, so it is sent as written.
    let request_2 = request_to(&broker, Method::GET, "temporal/entities?timerel=before&timeAt=2030-01-01T00%3A00%3A00Z")?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
