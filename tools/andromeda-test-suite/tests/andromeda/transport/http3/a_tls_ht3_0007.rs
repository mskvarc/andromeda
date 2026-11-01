//! A client trusting no authority fails the QUIC handshake.

use andromeda_test_suite::{
    config::BrokerRole,
    connection_outcome::ConnectionOutcome,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: a client speaking only HTTP/3 and trusting no authority fails the QUIC handshake, whose TLS 1.3 verifies
/// the broker's certificate as TCP's does, and gets no HTTP answer
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0007_a_client_trusting_no_authority_fails_the_quic_handshake() -> Result<()> {
    let untrusting = Connection::new()
        .trusting_nothing()
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    let answer = untrusting.with_accept(Accept::any()).source_identity().send().await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
