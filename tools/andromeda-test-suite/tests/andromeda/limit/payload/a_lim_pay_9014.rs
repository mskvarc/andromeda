//! A body at the limit is not refused for its size.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::spaces,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Create Entity with a body of exactly the request body limit is not answered 413, whatever its content
/// makes of it
/// Clause: 5.5.3, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 answers 413 to "too many bytes" without fixing a size; the
/// stack runs the default `http.max_request_body` of 64 `MiB`, and a refusal is answered before the upload is read
#[gtest]
#[tokio::test]
async fn a_lim_pay_9014_a_body_at_the_limit_is_not_refused_for_its_size() -> Result<()> {
    let broker = broker::front()?;
    let limit = SuiteConfig::get()?.budget(Budget::MaxRequestBody)?;
    // Whitespace of exactly the limit is a text no operation writes, refused for its content but not for its size.
    let answer = broker
        .send_unchecked(json_request_to(&broker, Method::POST, "entities", &spaces(limit)?)?)
        .await;
    expect_that!(answer.answered_status(), some(not(eq(StatusCode::PAYLOAD_TOO_LARGE))));
    Ok(())
}
