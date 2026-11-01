//! The terminator serves a client capped at TLS 1.2.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};
use reqwest::tls;

/// Pins: a client offering no TLS version newer than 1.2 completes the handshake with the terminator and is served by
/// the broker behind it
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker serves plaintext behind a TLS terminator and believes the forwarded fields the terminator sets; clause 6.2 requires TLS 1.2 to be supported, which falls to the terminator in this model
#[gtest]
#[tokio::test]
async fn a_tls_trm_0002_the_terminator_serves_a_client_capped_at_tls12() -> Result<()> {
    let terminator = Connection::new()
        .trusting("tls/ca.crt")
        .capped_at(tls::Version::TLS_1_2)
        .client(BrokerRole::Terminator)
        .await?;
    let headed = terminator.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-edge".parse::<ContextSourceAlias>()?));
    Ok(())
}
