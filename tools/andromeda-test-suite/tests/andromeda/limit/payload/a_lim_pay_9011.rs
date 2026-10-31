//! Twenty oversized uploads in a row each receive the refusal.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    generated_payload::spaces,
    problem::{problem, problem_status, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Twenty Create Entity requests in a row, each one octet over the request body limit and written without
/// waiting for 100 Continue, each answer 413 `RequestEntityTooLarge` naming the limit
/// Clause: 5.5.3, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 answers 413 to "too many bytes" without fixing a size; the
/// stack runs the default `http.max_request_body` of 64 `MiB`, and a refusal is answered before the upload is read
#[gtest]
#[tokio::test]
async fn a_lim_pay_9011_twenty_oversized_uploads_in_a_row_each_receive_the_refusal() -> Result<()> {
    let broker = broker::front()?;
    let limit = SuiteConfig::get()?.budget(Budget::MaxRequestBody)?;
    // The body is whitespace one octet over the limit, a text no operation writes and the size is the only reason to
    // refuse it.
    let body = spaces(limit + 1)?;
    for _ in 0..20 {
        let refused = broker.send_unchecked(json_request_to(&broker, Method::POST, "entities", &body)?).await;
        expect_that!(
            &refused,
            problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large()).with_detail_containing(&format!("{limit} bytes"))
        );
        expect_that!(problem_status(&refused), some(eq(413)));
    }
    Ok(())
}
