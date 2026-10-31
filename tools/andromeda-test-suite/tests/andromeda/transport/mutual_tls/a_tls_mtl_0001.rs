//! Optional client authentication serves a client without a certificate.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: with `tls.client_authentication` = optional, a client presenting no certificate completes the handshake
/// and is served
/// Clause: 6.2
/// Recorded: client certificate authentication is Andromeda's; clause 6.2 names TLS only
#[gtest]
#[tokio::test]
async fn a_tls_mtl_0001_optional_client_authentication_serves_a_client_without_a_certificate() -> Result<()> {
    let broker_tls_2 = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let identity = broker_tls_2.with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(&identity.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    Ok(())
}
