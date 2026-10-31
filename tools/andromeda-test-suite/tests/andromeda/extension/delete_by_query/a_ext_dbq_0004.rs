//! A window without a selector is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: a temporal delete-by-query carrying only a time window, which would reach every entity's history, answers
/// 400 `BadRequestData` as a too wide query
/// Clause: 5.6.21.4, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it and refuses a request without a type, attrs, q, georel or
/// local=true as clause 5.6.21.4 refuses a too wide Purge Entities
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0004_a_window_without_a_selector_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = request_to(&broker, Method::DELETE, "temporal/entities?timerel=before&timeAt=2030-01-01T00%3A00%3A00Z")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
