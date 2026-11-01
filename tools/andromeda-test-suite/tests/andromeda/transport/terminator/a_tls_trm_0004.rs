//! A context hosted through the terminator is stored under its https origin.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    tls_connection::Connection,
    tls_hosted_context::expect_https_url,
};
use googletest::prelude::*;
use http::Version;

/// Pins: a @context added through the terminator is stored under a URL with the terminator's https origin, and that
/// URL serves it
/// Clause: 5.13.2, 5.13.3, 5.13.4, 5.13.5, 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker serves plaintext behind a TLS terminator and believes the forwarded fields the terminator sets; with no advertised URL configured, the broker mints the URL from the forwarded scheme and host
#[gtest]
#[tokio::test]
async fn a_tls_trm_0004_a_context_hosted_through_the_terminator_is_stored_under_its_https_origin() -> Result<()> {
    let namespace = Namespace::fresh();
    let origin = SuiteConfig::get()?.broker(BrokerRole::Terminator)?;
    let terminator = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Terminator).await?;
    let headers: [(&str, &str); 0] = [];
    expect_https_url(&terminator, &namespace, &headers, Version::HTTP_2, origin).await
}
