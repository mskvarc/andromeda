//! A count through options is refused.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Context Source Registration Subscriptions with options=count answers 400 `InvalidRequest`
/// Clause: 6.3.11, 6.3.13, 6.3.20, Table 6.3.11-1
#[gtest]
#[tokio::test]
async fn n_css_qry_9005_a_count_through_options_is_refused() -> Result<()> {
    let broker = broker::front()?.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The client cannot write count as an options keyword, so it is sent as written.
    let request = request_to(&broker, Method::GET, "csourceSubscriptions?options=count")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
