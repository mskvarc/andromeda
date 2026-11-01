//! A client not trusting the stack authority fails the handshake.

use andromeda_test_suite::{config::BrokerRole, connection_outcome::ConnectionOutcome, tls_connection::Connection};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: a client trusting only an authority other than the one that issued the broker's certificate fails the TLS
/// handshake and gets no HTTP answer
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); the client trusts the stack's client authority beside the platform's, neither of which issued the
/// broker's certificate
#[gtest]
#[tokio::test]
async fn a_tls_tls_0013_a_client_not_trusting_the_stack_authority_fails_the_handshake() -> Result<()> {
    let stranger = Connection::new().trusting("tls/client-ca.crt").client(BrokerRole::Tls).await?;
    let answer = stranger.with_accept(Accept::any()).source_identity().send().await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
