//! A geo query with a four ordinate reference position is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: a temporal query whose `geoQ` reference position has four ordinates is refused with `BadRequestData`
/// Clause: 4.10, 5.7.4, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 says implementations "SHOULD NOT extend positions beyond three elements" and
/// that parsers "MAY ignore" extra elements, and 4.7.1 requires a valid `GeoJSON` geometry without saying what a
/// fourth element is, so the specification does not require the refusal; Andromeda refuses a position of four
/// ordinates as `BadRequestData` (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0010_a_geo_query_with_a_four_ordinate_reference_position_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Building_{namespace}&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&georel=near%3BmaxDistance%3D%3D100&geometry=Point&coordinates=%5B13.4%2C52.52%2C300.0%2C7.0%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
