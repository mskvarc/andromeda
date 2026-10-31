//! An id that is not a uri is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Delete Context Source Registration on broker A of an id that is not a URI answers 400 `BadRequestData`
/// Clause: 5.9.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_csr_del_7601_an_id_that_is_not_a_uri_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker, Method::DELETE, "csourceRegistrations/not-a-uri")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
