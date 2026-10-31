//! Forwarded fields claiming plaintext cannot downgrade the minted url.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    tls_connection::{Connection, HttpVersion},
    tls_hosted_context::expect_https_url,
};
use googletest::prelude::*;
use http::Version;

/// Pins: a @context added over TLS with X-Forwarded-Proto: http and Forwarded: proto=http is still stored under the
/// https origin the client used
/// Clause: 5.13.2, 5.13.3, 5.13.4, 5.13.5, 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); a broker terminating TLS itself trusts no forwarded field
/// (`http.forwarded.trust` is "none" by default)
#[gtest]
#[tokio::test]
async fn a_tls_tls_0008_forwarded_fields_claiming_plaintext_cannot_downgrade_the_minted_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let origin = SuiteConfig::get()?.broker(BrokerRole::Tls)?;
    let broker_tls = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http2)
        .client(BrokerRole::Tls)
        .await?;
    // The forwarded fields claim plaintext on a connection that is TLS.
    let headers = [("X-Forwarded-Proto", "http"), ("Forwarded", "proto=http")];
    expect_https_url(&broker_tls, &namespace, &headers, Version::HTTP_2, origin).await
}
