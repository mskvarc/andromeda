//! The native listener serves https over http2.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use http::Version;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: a request over TLS from a client offering h2 and http/1.1 is answered over HTTP/2 by the broker itself,
/// which names its configured identity
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); its TCP listener offers h2 before http/1.1 in ALPN
#[gtest]
#[tokio::test]
async fn a_tls_tls_0001_the_native_listener_serves_https_over_http2() -> Result<()> {
    let broker_tls_2 = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let headed = broker_tls_2.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.head.version(), eq(Version::HTTP_2));
    let identity = headed.outcome;
    expect_that!(&identity.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    Ok(())
}
