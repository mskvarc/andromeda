//! A limit that is no number is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Context Source Registration Subscriptions with limit abc answers 400 `BadRequestData`
/// Clause: 5.11.5.4, 5.5.9, 6.3.10
#[gtest]
#[tokio::test]
async fn n_css_qry_0002_a_limit_that_is_no_number_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "csourceSubscriptions?limit=abc&offset=0",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
