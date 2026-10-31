//! A snapshot body of another media type or of no bytes is refused.

use andromeda_test_suite::{
    broker,
    problem::{answered, problem},
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_LENGTH};
use mediatype::{MediaType, media_type};
use ngsi_ld::model::{APPLICATION_JSON, ErrorType};

/// The media type the refused body is sent as.
const TEXT_PLAIN: MediaType<'static> = media_type!(TEXT / PLAIN);

/// Pins: Create Snapshot answers 415 for a text/plain body and 400 `InvalidRequest` for an empty application/json
/// body
/// Clause: 6.3.4, 6.3.2, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8123_a_snapshot_body_of_another_media_type_or_of_no_bytes_is_refused() -> Result<()> {
    let broker = broker::front()?;
    // A body of another media type and an empty body are refusals the test checks, which no typed Snapshot writes.
    let request = content_type(request_to(&broker, Method::POST, "snapshots")?, &TEXT_PLAIN)?.body(Bytes::from_static(b"payload"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    let request_2 =
        content_type(request_to(&broker, Method::POST, "snapshots")?, &APPLICATION_JSON)?.append_header(CONTENT_LENGTH, HeaderValue::from_static("0"));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
