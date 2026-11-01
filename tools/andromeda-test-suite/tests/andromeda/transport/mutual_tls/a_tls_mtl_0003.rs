//! Optional client authentication refuses a certificate from an unknown issuer.

use andromeda_test_suite::{config::BrokerRole, connection_outcome::ConnectionOutcome, tls_connection::Connection};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: with `tls.client_authentication` = optional, a client presenting a self-signed certificate from an issuer
/// outside the configured client CA fails the handshake and gets no HTTP answer
/// Clause: 6.2
/// Recorded: client certificate authentication is Andromeda's; clause 6.2 names TLS only; under TLS 1.3 the broker verifies a client certificate after the client has finished its side of the
/// handshake, so the refusal reaches the client as an alert or a closed connection in place of an answer
#[gtest]
#[tokio::test]
async fn a_tls_mtl_0003_optional_client_authentication_refuses_a_certificate_from_an_unknown_issuer() -> Result<()> {
    let client = Connection::new()
        .trusting("tls/ca.crt")
        .presenting("tls/stranger.crt", "tls/stranger.key")
        .client(BrokerRole::Tls)
        .await?;
    let answer = client.with_accept(Accept::any()).source_identity().send().await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
