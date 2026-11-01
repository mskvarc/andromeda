//! A client capped at TLS 1.2 is served by the native listener.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};
use reqwest::tls;

/// Pins: a client offering no TLS version newer than 1.2 completes the handshake with the broker's own TLS listener
/// under the default floor and is served
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); `tls.minimum_protocol_version` is "tls1.2" by default
#[gtest]
#[tokio::test]
async fn a_tls_tls_0010_a_client_capped_at_tls12_is_served_by_the_native_listener() -> Result<()> {
    let broker_tls = Connection::new()
        .trusting("tls/ca.crt")
        .capped_at(tls::Version::TLS_1_2)
        .client(BrokerRole::Tls)
        .await?;
    let headed = broker_tls.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    Ok(())
}
