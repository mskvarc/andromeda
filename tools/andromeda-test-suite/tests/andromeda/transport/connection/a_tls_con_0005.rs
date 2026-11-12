//! An upload declaring text plain is answered 415, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion, answered_version},
    generated_payload::spaces,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use strum::IntoEnumIterator;

/// Pins: Create Entity of 16 `MiB` declaring text/plain is answered a bare 415 before the broker reads the content,
/// the answer carrying `Connection: close` over HTTP/1.1 and no Connection field over HTTP/2
/// Clause: 6.3.4
/// Recorded: HTTP binding, not v1.9.1: RFC 9110 section 10.1.1 has a server that answers before reading the content
/// say whether it closes the connection, over HTTP/1.1 closing is the only way to stop the upload (RFC 9112 section
/// 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0005_an_upload_declaring_text_plain_is_answered_415_announcing_the_close_over_http11_only() -> Result<()> {
    let content = Bytes::from(spaces(REFUSED_UPLOAD)?);
    for version in RefusedVersion::iter() {
        let broker_tls = version.client().await?;
        // The content goes as text/plain, which no typed operation sends.
        let request = request_to(&broker_tls, Method::POST, "entities")?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("text/plain"))
            .body(content.clone());
        let answer = broker_tls.send_unchecked(request).await;
        expect_that!(&answer, version.announcing(answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body()));
        expect_that!(answered_version(&answer), some(eq(version.version())));
    }
    Ok(())
}
