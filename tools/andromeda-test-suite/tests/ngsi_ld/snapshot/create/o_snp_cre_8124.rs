//! A create without a length is 411 and the media refusals have no body.

use andromeda_test_suite::{
    broker,
    problem::answered,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use mediatype::{MediaType, media_type};

/// The media type the refused body is sent as.
const TEXT_PLAIN: MediaType<'static> = media_type!(TEXT / PLAIN);

/// Pins: Create Snapshot without a body or a Content-Length answers 411 with no body, and a text/plain body answers
/// 415 with no body
/// Clause: 6.3.4, 5.16.1.4
/// Recorded: clause 6.3.4 refuses a body of a media type the operation does not take and names no status for a
/// request without a length; Andromeda answers 411 Length Required, and answers both refusals without a body
#[gtest]
#[tokio::test]
async fn o_snp_cre_8124_a_create_without_a_length_is_411_and_the_media_refusals_have_no_body() -> Result<()> {
    let broker = broker::front()?;
    // No typed operation sends a create without a body and a length, so the request is written as given.
    let request = request_to(&broker, Method::POST, "snapshots")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::LENGTH_REQUIRED).without_body());
    // The body is no JSON and goes as text/plain, which no typed operation sends, so the request is written as given.
    let request_2 = content_type(request_to(&broker, Method::POST, "snapshots")?, &TEXT_PLAIN)?.body(Bytes::from_static(b"payload"));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    Ok(())
}
