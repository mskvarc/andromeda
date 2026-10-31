//! The management listener answers readiness over tls.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    presented_certificate::PresentedCertificate,
    problem::answered,
    tls_connection::Connection,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: a separate-port management listener with management.transport = tls answers readyz over TLS, presenting
/// the same certificate as the API listener
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); the management listener is Andromeda's own and shares the
/// broker's TLS identity
#[gtest]
#[tokio::test]
async fn a_tls_tls_0005_the_management_listener_answers_readiness_over_tls() -> Result<()> {
    let management = Connection::new().trusting("tls/ca.crt").client(BrokerRole::TlsManagement).await?;
    // The readiness probe sits at the root of the management listener, outside the API root every operation targets.
    let probe = SuiteConfig::get()?.broker(BrokerRole::TlsManagement)?.join("readyz")?;
    let answer = management
        .send_unchecked(request_to(&management, Method::GET, "")?.uri(probe.as_str().parse()?))
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let certificate = PresentedCertificate::of(answer?.extensions())?;
    expect_that!(certificate.serial_number(), eq("db:64:23:56:e6:77:56:79:a7:91:cd:77:f6:37:ca:66:bb:b5:11"));
    Ok(())
}
