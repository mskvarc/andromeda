//! A creation without any narrowing input is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with a temporal query and no selector of
/// types, attributes, query or geoquery answers 400 `BadRequestData` as too wide
/// Clause: 5.14.5.4
#[gtest]
#[tokio::test]
async fn n_map_qte_7001_a_creation_without_any_narrowing_input_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "temporal/entityMaps?timerel=before&timeAt=2022-01-01T00%3A00%3A00Z",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
