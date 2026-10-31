//! A posted query without a narrowing member is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create `EntityMap` for Query Entities through POST `/entityMaps` with a Query holding only its type
/// answers 400 `BadRequestData` as too wide
/// Clause: 5.14.4.4, 6.34.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_0008_a_posted_query_without_a_narrowing_member_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityMaps")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"type\": \"Query\"\n}"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    Ok(())
}
