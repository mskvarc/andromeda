//! Create entity without a length is 411 without a body.

use andromeda_test_suite::{config::BrokerRole, raw_http::send_bytes};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Create Entity declaring application/json and sending neither a body nor a Content-Length answers 411 with
/// no body
/// Clause: 6.3.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9008_create_entity_without_a_length_is_411_without_a_body() -> Result<()> {
    // The request states neither a body nor a Content-Length, which the HTTP client completes with a zero length, so it
    // is written to the socket as is.
    let response = send_bytes(BrokerRole::Front, &Method::POST, "entities", &[("Content-Type", "application/json")], b"").await?;
    expect_that!(response.status(), eq(StatusCode::LENGTH_REQUIRED));
    expect_that!(response.body().len(), eq(0));
    Ok(())
}
