//! The connection ends cleanly after the refusal.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::spaces,
    problem::{problem, problem_status, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONNECTION};

/// Pins: Add @context one octet over the request body limit, written without waiting for 100 Continue, is answered
/// 413 with Connection close and the whole problem detail, the connection ending without a reset
/// Clause: 5.5.3, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 answers 413 to "too many bytes" without fixing a size; the
/// stack runs the default `http.max_request_body` of 64 `MiB`, and a refusal is answered before the upload is read
///
/// A reset under the unread upload would fail the request in curl before any assertion.
#[gtest]
#[tokio::test]
async fn a_lim_pay_9013_the_connection_ends_cleanly_after_the_refusal() -> Result<()> {
    let broker = broker::front()?;
    let limit = SuiteConfig::get()?.budget(Budget::MaxRequestBody)?;
    // The body is whitespace one octet over the limit, a text no operation writes and the size is the only reason to
    // refuse it.
    let refused = broker
        .send_unchecked(json_request_to(&broker, Method::POST, "jsonldContexts", &spaces(limit + 1)?)?)
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&format!("{limit} bytes"))
            .with_header(CONNECTION, "close")
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    Ok(())
}
