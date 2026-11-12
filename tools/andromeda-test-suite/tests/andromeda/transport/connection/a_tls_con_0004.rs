//! An upload declaring no length is answered 411, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion},
    streamed_upload::{StreamedContent, send_streamed},
    wire::header_value,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONNECTION, CONTENT_TYPE},
};
use strum::IntoEnumIterator;

/// Pins: Create Entity streaming 16 `MiB` of content with no Content-Length is answered a bare 411 before the broker
/// reads the content, the answer carrying `Connection: close` over HTTP/1.1 and no Connection field over HTTP/2
/// Clause: 6.3.4
/// Recorded: HTTP binding, not v1.9.1: RFC 9110 section 10.1.1 has a server that answers before reading the content
/// say whether it closes the connection, over HTTP/1.1 closing is the only way to stop the upload (RFC 9112 section
/// 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0004_an_upload_declaring_no_length_is_answered_411_announcing_the_close_over_http11_only() -> Result<()> {
    let headers = [(CONTENT_TYPE, HeaderValue::from_static("application/json"))];
    for version in RefusedVersion::iter() {
        let broker_tls = version.raw_client().await?;
        // The typed client declares the length of every body it sends, so the content goes out as a stream.
        let answer = send_streamed(&broker_tls, Method::POST, "entities", &headers, StreamedContent::Undeclared(REFUSED_UPLOAD)).await?;
        expect_that!(answer.status(), eq(StatusCode::LENGTH_REQUIRED));
        expect_that!(answer.version(), eq(version.version()));
        expect_that!(header_value(answer.headers(), &CONNECTION), eq(version.connection_field()));
        expect_that!(answer.body().len(), eq(0));
    }
    Ok(())
}
