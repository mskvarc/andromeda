//! Retrieve available attributes with an unknown parameter is invalid request.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Available Attributes with an unknown query parameter answers 400 `InvalidRequest`
/// Clause: 6.3.2, 5.7.8
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_0002_retrieve_available_attributes_with_an_unknown_parameter_is_invalid_request() -> Result<()> {
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "attributes?invalidParam=invalidValue",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
