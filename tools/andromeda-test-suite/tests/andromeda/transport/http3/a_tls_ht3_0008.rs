//! A client offering only h2 fails the QUIC handshake.

use andromeda_test_suite::{
    config::BrokerRole,
    connection_outcome::ConnectionOutcome,
    quic_handshake::{ApplicationProtocol, quic_handshake},
};
use googletest::prelude::*;

/// Pins: a QUIC handshake offering only the `h2` application protocol fails, while one offering `h3` to the same
/// listener completes; the listener selects `h3` or nothing (IETF RFC 9001 section 8.1)
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0008_a_client_offering_only_h2_fails_the_quic_handshake() -> Result<()> {
    let h2 = quic_handshake(BrokerRole::Tls, "tls/ca.crt", &[ApplicationProtocol::Http2]).await?;
    expect_that!(h2, eq(ConnectionOutcome::Refused));
    let h3 = quic_handshake(BrokerRole::Tls, "tls/ca.crt", &[ApplicationProtocol::Http3]).await?;
    expect_that!(h3, eq(ConnectionOutcome::Accepted));
    Ok(())
}
