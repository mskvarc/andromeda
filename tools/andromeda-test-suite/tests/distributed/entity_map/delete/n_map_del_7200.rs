//! An identifier that is no uri is bad request data.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Delete `EntityMap` with an identifier that is not a URI answers 400 `BadRequestData`
/// Clause: 5.14.3.4, 6.33.3.1
#[gtest]
#[tokio::test]
async fn n_map_del_7200_an_identifier_that_is_no_uri_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The typed delete takes an `EntityMap` identifier, which the identifier of this request is not, so it is sent as
    // written.
    let request = request_to(&broker, Method::DELETE, "entityMaps/not%20a%20uri")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
