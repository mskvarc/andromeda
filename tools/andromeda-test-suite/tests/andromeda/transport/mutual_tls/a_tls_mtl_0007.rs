//! Required client authentication serves a certificate from the trusted authority.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: with `tls.client_authentication` = required, a client presenting a certificate issued by the configured
/// client CA completes the handshake and is served
/// Clause: 6.2
/// Recorded: client certificate authentication is Andromeda's; clause 6.2 names TLS only
#[gtest]
#[tokio::test]
async fn a_tls_mtl_0007_required_client_authentication_serves_a_certificate_from_the_trusted_authority() -> Result<()> {
    let authorized = Connection::new()
        .trusting("tls/ca.crt")
        .presenting("tls/client.crt", "tls/client.key")
        .client(BrokerRole::Mutual)
        .await?;
    let headed = authorized.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-mutual".parse::<ContextSourceAlias>()?));
    Ok(())
}
