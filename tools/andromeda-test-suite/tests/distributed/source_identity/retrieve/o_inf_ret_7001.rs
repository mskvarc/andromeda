//! A query parameter the operation does not take is invalid request.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Context Source Identity Information with a query parameter answers 400 `InvalidRequest`
/// Clause: 5.15.1.3, 6.3.2
/// Recorded: 5.15.1.3 lists no input data, and 6.3.20 says only that an incompatible parameter "should" give
/// `InvalidRequest`, so rejecting rather than ignoring it is the broker's choice
#[gtest]
#[tokio::test]
async fn o_inf_ret_7001_a_query_parameter_the_operation_does_not_take_is_invalid_request() -> Result<()> {
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        "info/sourceIdentity?ignored=value",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
