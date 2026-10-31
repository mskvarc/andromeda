//! Every batch operation without a length is 411 without a body.

use andromeda_test_suite::{config::BrokerRole, raw_http::send_bytes};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Batch create, upsert, update, merge and delete and the Query Entities POST binding sending neither a body
/// nor a Content-Length each answer 411 with no body
/// Clause: 6.3.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9009_every_batch_operation_without_a_length_is_411_without_a_body() -> Result<()> {
    // Each request states neither a body nor a Content-Length, which the HTTP client completes with a zero length, so
    // it is written to the socket as is.
    for operation in ["create", "upsert", "update", "merge", "delete", "query"] {
        let response = send_bytes(
            BrokerRole::Front,
            &Method::POST,
            &format!("entityOperations/{operation}"),
            &[("Content-Type", "application/json")],
            b"",
        )
        .await?;
        expect_that!((operation, response.status()), eq((operation, StatusCode::LENGTH_REQUIRED)));
        expect_that!((operation, response.body().len()), eq((operation, 0)));
    }
    Ok(())
}
