//! An upload whose header section is over the limit is answered 431, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion, answered_version},
    generated_payload::spaces,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use strum::IntoEnumIterator;

/// The length of the one field that takes the header section past the 32 `KiB` limit.
const FILL: usize = 40 * 1024;

/// Pins: Create Entity of 16 `MiB` whose header section carries a 40 `KiB` field is answered 431 without a body before
/// the broker reads the content, the answer carrying `Connection: close` over HTTP/1.1 and no Connection field over
/// HTTP/2
/// Clause: 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: the stack runs the default `server.max_request_header_section` of
/// 32 `KiB`, and RFC 6585 section 5 answers a section over it 431; RFC 9110 section 10.1.1 has a server that answers
/// before reading the content say whether it closes the connection, over HTTP/1.1 closing is the only way to stop the
/// upload (RFC 9112 section 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0012_an_upload_whose_header_section_is_over_the_limit_is_answered_431_announcing_the_close_over_http11_only() -> Result<()> {
    let content = spaces(REFUSED_UPLOAD)?;
    let fill = HeaderValue::from_str(&"a".repeat(FILL))?;
    for version in RefusedVersion::iter() {
        let broker_tls = version.client().await?;
        // No typed operation sends a field of its own.
        let request = json_request_to(&broker_tls, Method::POST, "entities", &content)?.append_header(HeaderName::from_static("x-fill"), fill.clone());
        let answer = broker_tls.send_unchecked(request).await;
        expect_that!(
            &answer,
            version.announcing(answered(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE).without_body())
        );
        expect_that!(answered_version(&answer), some(eq(version.version())));
    }
    Ok(())
}
