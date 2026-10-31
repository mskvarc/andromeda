//! A temporal map creation without narrowing is too wide.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with only timerel and `timeAt` answers 400
/// `BadRequestData`
/// Clause: 5.7.4.4, 5.14.5.4
#[gtest]
#[tokio::test]
async fn n_map_qte_9005_a_temporal_map_creation_without_narrowing_is_too_wide() -> Result<()> {
    let broker = broker::front()?;
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        "temporal/entityMaps?timerel=before&timeAt=2099-01-01T00%3A00%3A00Z",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
