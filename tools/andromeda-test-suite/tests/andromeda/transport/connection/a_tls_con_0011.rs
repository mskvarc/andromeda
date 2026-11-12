//! An upload that stops arriving is answered 408, announcing the close over http11 only.

use andromeda_test_suite::{
    early_refusal::{REFUSED_UPLOAD, RefusedVersion},
    streamed_upload::{StreamedContent, StreamedUploadError, send_streamed},
    wire::header_value,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONNECTION, CONTENT_TYPE},
};
use ngsi_ld::model::{ErrorType, ProblemDetails};
use strum::IntoEnumIterator;

/// Pins: Create Entity declaring 16 `MiB` of content and sending one chunk of it is answered, once the broker's
/// request deadline passes, 408 with a `RequestTimeout` problem document, the answer carrying `Connection: close`
/// over HTTP/1.1 and no Connection field over HTTP/2
/// Clause: 6.3.2
/// Recorded: the client failed to send its request in time, which IETF RFC 9110 clause 15.5.9 answers 408; Table
/// 6.3.2-1 names no error type for 408, so `RequestTimeout` is Andromeda's own; RFC 9110 section 10.1.1 has a server
/// that answers before reading the content say whether it closes the connection, over HTTP/1.1 closing is the only
/// way to stop the upload (RFC 9112 section 9.6), and RFC 9113 section 8.2.2 forbids the field over HTTP/2
#[gtest]
#[tokio::test]
async fn a_tls_con_0011_an_upload_that_stops_arriving_is_answered_408_announcing_the_close_over_http11_only() -> Result<()> {
    let request_timeout = "https://uri.etsi.org/ngsi-ld/errors/RequestTimeout".parse::<ErrorType>()?;
    let headers = &[(CONTENT_TYPE, HeaderValue::from_static("application/json"))];
    // Both versions wait out the same request deadline, so they wait together.
    let answers = join_all(RefusedVersion::iter().map(|version| async move {
        let broker_tls = version.raw_client().await?;
        // The typed client sends the whole body it declares, so the content goes out as a stream that stops.
        let answer = send_streamed(&broker_tls, Method::POST, "entities", headers, StreamedContent::Stalled(REFUSED_UPLOAD)).await?;
        Ok::<_, StreamedUploadError>((version, answer))
    }))
    .await;
    for answered in answers {
        let (version, answer) = answered?;
        expect_that!(answer.status(), eq(StatusCode::REQUEST_TIMEOUT));
        expect_that!(answer.version(), eq(version.version()));
        expect_that!(header_value(answer.headers(), &CONNECTION), eq(version.connection_field()));
        let details = serde_json::from_slice::<ProblemDetails>(answer.body())?;
        expect_that!(details.error_type, some(eq(&request_timeout)));
        expect_that!(details.status, some(eq(408)));
    }
    Ok(())
}
