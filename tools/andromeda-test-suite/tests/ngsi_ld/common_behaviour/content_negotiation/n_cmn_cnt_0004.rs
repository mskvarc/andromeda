//! Create entity declaring text plain is 415.

use andromeda_test_suite::{broker, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};

/// Pins: Create Entity with Content-Type text/plain answers 415
/// Clause: 6.3.4, 5.6.1
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_0004_create_entity_declaring_text_plain_is_415() -> Result<()> {
    let broker = broker::front()?;
    // The body is not JSON, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("text/plain"))
        .body(Bytes::from("not json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    Ok(())
}
