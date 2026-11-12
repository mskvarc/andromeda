//! An upload to an entity id that is no uri is answered 400, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion, answered_version},
    generated_payload::spaces,
    problem::problem,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;
use strum::IntoEnumIterator;

/// Pins: Update Attributes of 16 `MiB` addressed to an Entity id that is not a URI is answered 400 `BadRequestData`
/// before the broker reads the content, the answer carrying `Connection: close` over HTTP/1.1 and no Connection field
/// over HTTP/2
/// Clause: 5.6.2.4, 6.6.3.3
/// Recorded: HTTP binding, not v1.9.1: RFC 9110 section 10.1.1 has a server that answers before reading the content
/// say whether it closes the connection, over HTTP/1.1 closing is the only way to stop the upload (RFC 9112 section
/// 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0007_an_upload_to_an_entity_id_that_is_no_uri_is_answered_400_announcing_the_close_over_http11_only() -> Result<()> {
    let content = spaces(REFUSED_UPLOAD)?;
    for version in RefusedVersion::iter() {
        let broker_tls = version.client().await?;
        // The typed client addresses an Entity only by a URI, so the path is written as is.
        let request = json_request_to(&broker_tls, Method::PATCH, "entities/not-a-uri/attrs", &content)?;
        let answer = broker_tls.send_unchecked(request).await;
        expect_that!(&answer, version.announcing(problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)));
        expect_that!(answered_version(&answer), some(eq(version.version())));
    }
    Ok(())
}
