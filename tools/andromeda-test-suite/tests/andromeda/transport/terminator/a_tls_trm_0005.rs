//! The terminator replaces a forwarded scheme the client sent.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    namespace::Namespace,
    tls_connection::Connection,
    tls_hosted_context::expect_https_url,
};
use googletest::prelude::*;
use http::Version;

/// Pins: a @context added through the terminator with X-Forwarded-Proto: http and Forwarded: proto=http is still
/// stored under the terminator's https origin: the terminator sets the forwarded fields in place of the client's
/// Clause: 5.13.2, 5.13.3, 5.13.4, 5.13.5, 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker serves plaintext behind a TLS terminator and believes the forwarded fields the terminator sets; the broker believes forwarded fields from any address, so only the terminator replacing them keeps
/// a client from choosing the scheme
#[gtest]
#[tokio::test]
async fn a_tls_trm_0005_the_terminator_replaces_a_forwarded_scheme_the_client_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let origin = SuiteConfig::get()?.broker(BrokerRole::Terminator)?;
    let terminator = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Terminator).await?;
    let headers = [("X-Forwarded-Proto", "http"), ("Forwarded", "proto=http")];
    expect_https_url(&terminator, &namespace, &headers, Version::HTTP_2, origin).await
}
