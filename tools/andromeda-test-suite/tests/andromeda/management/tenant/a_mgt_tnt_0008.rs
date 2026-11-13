//! Listing tenants with an unknown parameter is invalid request.

use andromeda_test_suite::{broker, problem::problem, raw_call::target, wire::andromeda_request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: GET /ngsi-ld/andromeda/v1/tenants with an unknown query parameter answers 400 `InvalidRequest`
/// Clause: 6.3.2
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0008_listing_tenants_with_an_unknown_parameter_is_invalid_request() -> Result<()> {
    let front = broker::front()?;
    // The Tenant collection is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request = andromeda_request_to(&front, Method::GET, &target("tenants", &[("invalidParam", "invalidValue")]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
