//! Over http2 the refusal carries no connection header.

use andromeda_test_suite::{
    config::{BrokerRole, Budget, SuiteConfig},
    generated_payload::spaces,
    problem::{problem, request_entity_too_large},
    tls_connection::{Connection, HttpVersion},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, Version, header::CONNECTION};
use ngsi_ld::{client::ClientError, http::ResponseHead};

/// Pins: Create Entity over HTTP/2 one octet over the request body limit answers 413 `RequestEntityTooLarge` naming
/// the limit, without a Connection header (RFC 9113 section 8.2.2)
/// Clause: 5.5.3, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 answers 413 to "too many bytes" without fixing a size; the
/// stack runs the default `http.max_request_body` of 64 `MiB`, and a refusal is answered before the upload is read
#[gtest]
#[tokio::test]
async fn a_lim_pay_9012_over_http2_the_refusal_carries_no_connection_header() -> Result<()> {
    let limit = SuiteConfig::get()?.budget(Budget::MaxRequestBody)?;
    let broker_tls = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http2)
        .client(BrokerRole::Tls)
        .await?;
    // The body is whitespace one octet over the limit, so the size is the only reason to refuse it.
    let request = json_request_to(&broker_tls, Method::POST, "entities", &spaces(limit + 1)?)?;
    let answer = broker_tls.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&format!("{limit} bytes"))
            .without_header(CONNECTION)
    );
    let refusal = answer.as_ref().err();
    expect_that!(refusal.and_then(ClientError::problem).and_then(|details| details.status), some(eq(413)));
    expect_that!(refusal.and_then(ClientError::head).map(ResponseHead::version), some(eq(Version::HTTP_2)));
    Ok(())
}
