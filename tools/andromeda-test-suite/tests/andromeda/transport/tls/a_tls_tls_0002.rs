//! An http11 only client is served.

use andromeda_test_suite::{
    config::BrokerRole,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use http::Version;
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: a client offering only http/1.1 over TLS is served over HTTP/1.1
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls)
#[gtest]
#[tokio::test]
async fn a_tls_tls_0002_an_http11_only_client_is_served() -> Result<()> {
    let broker_tls_2 = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http11)
        .client(BrokerRole::Tls)
        .await?;
    let headed = broker_tls_2.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.head.version(), eq(Version::HTTP_11));
    let identity = headed.outcome;
    expect_that!(&identity.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    Ok(())
}
