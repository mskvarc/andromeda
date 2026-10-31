//! Near with both min and max distance is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Georel=near with both a `minDistance` and a `maxDistance` modifier is `BadRequestData`
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0027_near_with_both_min_and_max_distance_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // 4.10: nearRel = nearOp andOp distance equal PositiveNumber admits one modifier; 5.7.2.4 makes a syntactically invalid geoquery BadRequestData.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Spot_{namespace}&georel=near%3BminDistance%3D%3D500%3BmaxDistance%3D%3D10000&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
