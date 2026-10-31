//! The presented leaf is the configured certificate.

use andromeda_test_suite::{config::BrokerRole, presented_certificate::PresentedCertificate, tls_connection::Connection};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: the TLS listener presents the leaf certificate it is configured with: subject CN=localhost, issued by the
/// stack CA, with that certificate's serial number
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); the serial number is that of the stack's broker.crt
#[gtest]
#[tokio::test]
async fn a_tls_tls_0003_the_presented_leaf_is_the_configured_certificate() -> Result<()> {
    let broker_tls = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let headed = broker_tls.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    let certificate = PresentedCertificate::of(headed.head.extensions())?;
    expect_that!(certificate.subject(), eq("CN = localhost"));
    expect_that!(certificate.issuer(), eq("CN = Andromeda TLS stack CA"));
    expect_that!(certificate.serial_number(), eq("db:64:23:56:e6:77:56:79:a7:91:cd:77:f6:37:ca:66:bb:b5:11"));
    Ok(())
}
