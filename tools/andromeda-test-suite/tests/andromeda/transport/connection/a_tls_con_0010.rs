//! An upload in a coding the broker does not decode is answered 415, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion, answered_version},
    generated_payload::spaces,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_ENCODING};
use strum::IntoEnumIterator;

/// Pins: Create Entity of 16 `MiB` declaring `Content-Encoding: compress`, a coding the broker does not decode, is
/// answered 415 before the broker reads the content, the answer carrying `Connection: close` over HTTP/1.1 and no
/// Connection field over HTTP/2
/// Clause: 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: the stack runs the default `http.decompression`, which decodes
/// gzip, deflate, br and zstd and refuses any other coding with 415 (RFC 9110 section 15.5.16); RFC 9110 section
/// 10.1.1 has a server that answers before reading the content say whether it closes the connection, over HTTP/1.1
/// closing is the only way to stop the upload (RFC 9112 section 9.6), and RFC 9113 section 8.2.2 forbids the field
/// over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0010_an_upload_in_a_coding_the_broker_does_not_decode_is_answered_415_announcing_the_close_over_http11_only() -> Result<()> {
    let content = spaces(REFUSED_UPLOAD)?;
    for version in RefusedVersion::iter() {
        let broker_tls = version.client().await?;
        // No typed operation codes its body.
        let request = json_request_to(&broker_tls, Method::POST, "entities", &content)?.append_header(CONTENT_ENCODING, HeaderValue::from_static("compress"));
        let answer = broker_tls.send_unchecked(request).await;
        expect_that!(&answer, version.announcing(answered(StatusCode::UNSUPPORTED_MEDIA_TYPE)));
        expect_that!(answered_version(&answer), some(eq(version.version())));
    }
    Ok(())
}
