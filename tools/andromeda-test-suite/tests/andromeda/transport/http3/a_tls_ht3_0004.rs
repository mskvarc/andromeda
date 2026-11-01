//! A trailing slash is normalised over HTTP/3.

use andromeda_test_suite::{
    config::BrokerRole,
    tls_connection::{Connection, HttpVersion},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, Version};

/// Pins: GET /ngsi-ld/v1/health/ from a client speaking only HTTP/3 reaches the health endpoint, as the path without
/// the trailing slash does over TCP
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener; the broker trims one trailing slash before routing, whatever the transport
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0004_a_trailing_slash_is_normalised_over_http3() -> Result<()> {
    let quic = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    // The target with a trailing slash is the subject, so the request is sent as written.
    let answer = quic.send_unchecked(request_to(&quic, Method::GET, "health/")?).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(answer.version(), eq(Version::HTTP_3));
    Ok(())
}
