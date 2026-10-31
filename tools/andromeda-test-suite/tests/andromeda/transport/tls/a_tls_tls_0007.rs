//! A context hosted over http11 is stored under an https url.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    tls_connection::{Connection, HttpVersion},
    tls_hosted_context::expect_https_url,
};
use googletest::prelude::*;
use http::Version;

/// Pins: a @context added over HTTP/1.1 on the TLS listener is stored under a URL with the https origin the client
/// used, and that URL serves it
/// Clause: 5.13.2, 5.13.3, 5.13.4, 5.13.5, 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); with no advertised URL configured, the broker mints the
/// URL from the scheme of the listener and the Host the client sent
#[gtest]
#[tokio::test]
async fn a_tls_tls_0007_a_context_hosted_over_http11_is_stored_under_an_https_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let origin = SuiteConfig::get()?.broker(BrokerRole::Tls)?;
    let broker_tls = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http11)
        .client(BrokerRole::Tls)
        .await?;
    let headers: [(&str, &str); 0] = [];
    expect_https_url(&broker_tls, &namespace, &headers, Version::HTTP_11, origin).await
}
