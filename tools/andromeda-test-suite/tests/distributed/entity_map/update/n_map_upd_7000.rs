//! An identifier that is no uri is bad request data.

use andromeda_test_suite::{broker, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Update `EntityMap` of an identifier that is not a URI answers 400 `BadRequestData`
/// Clause: 5.14.2.4, 6.32.3.2
#[gtest]
#[tokio::test]
async fn n_map_upd_7000_an_identifier_that_is_no_uri_is_bad_request_data() -> Result<()> {
    let broker = broker::front()?;
    // The client takes an `EntityMapId`, which refuses the identifier, so the request is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        "entityMaps/not-a-valid-uri",
        "{\n    \"expiresAt\": \"2099-01-01T00:00:00Z\"\n}",
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
