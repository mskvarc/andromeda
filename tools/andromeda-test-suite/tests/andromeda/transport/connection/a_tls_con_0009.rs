//! An upload with a method the resource does not serve is answered 405, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion, answered_version},
    generated_payload::spaces,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::ALLOW};
use strum::IntoEnumIterator;

/// Pins: a PUT of 16 `MiB` to the `entities/` resource, which serves GET and POST, is answered 405 naming both in its
/// Allow header before the broker reads the content, the answer carrying `Connection: close` over HTTP/1.1 and no
/// Connection field over HTTP/2
/// Clause: 6.3.2, 6.4.3
/// Recorded: HTTP binding, not v1.9.1: RFC 9110 section 10.1.1 has a server that answers before reading the content
/// say whether it closes the connection, over HTTP/1.1 closing is the only way to stop the upload (RFC 9112 section
/// 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0009_an_upload_with_a_method_the_resource_does_not_serve_is_answered_405_announcing_the_close_over_http11_only() -> Result<()> {
    let content = spaces(REFUSED_UPLOAD)?;
    for version in RefusedVersion::iter() {
        let broker_tls = version.client().await?;
        // No typed operation puts to the `entities/` resource.
        let request = json_request_to(&broker_tls, Method::PUT, "entities", &content)?;
        let answer = broker_tls.send_unchecked(request).await;
        expect_that!(
            &answer,
            version.announcing(
                answered(StatusCode::METHOD_NOT_ALLOWED)
                    .with_header_containing(ALLOW, "GET")
                    .with_header_containing(ALLOW, "POST")
            )
        );
        expect_that!(answered_version(&answer), some(eq(version.version())));
    }
    Ok(())
}
