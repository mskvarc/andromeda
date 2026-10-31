//! A query of every type with local false is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Query Entities with type * and local false answers 400 `InvalidRequest`
/// Clause: Table 6.4.3.2-1, 5.2.33, 6.3.20
/// Recorded: clause 5.2.33 forbids setting local scope to false for type * and clause 6.3.20 says only that an
/// `InvalidRequest` should be returned for incompatible parameters
#[gtest]
#[tokio::test]
async fn o_fed_qry_7203_a_query_of_every_type_with_local_false_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client refuses every type with local=false before sending, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities?type=%2A&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&local=false"),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
