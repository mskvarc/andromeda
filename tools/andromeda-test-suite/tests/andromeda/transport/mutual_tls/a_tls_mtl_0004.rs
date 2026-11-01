//! A TLS 1.3 floor refuses an authorized TLS 1.2 client.

use andromeda_test_suite::{config::BrokerRole, connection_outcome::ConnectionOutcome, tls_connection::Connection};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;
use reqwest::tls;

/// Pins: with `tls.minimum_protocol_version` = tls1.3, a client offering no TLS version newer than 1.2 fails the
/// handshake although it presents a certificate of the configured client CA
/// Clause: 6.2
/// Recorded: the TLS floor is Andromeda's; clause 6.2 requires TLS 1.2 to be supported, which a deployment raising
/// the floor opts out of
#[gtest]
#[tokio::test]
async fn a_tls_mtl_0004_a_tls13_floor_refuses_an_authorized_tls12_client() -> Result<()> {
    let client = Connection::new()
        .trusting("tls/ca.crt")
        .presenting("tls/client.crt", "tls/client.key")
        .capped_at(tls::Version::TLS_1_2)
        .client(BrokerRole::Mutual)
        .await?;
    let answer = client.with_accept(Accept::any()).source_identity().send().await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
