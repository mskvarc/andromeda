//! A plaintext request to the TLS management listener gets no HTTP answer.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    connection_outcome::ConnectionOutcome,
    wire::request_to,
};
use googletest::prelude::*;
use http::Method;

/// Pins: a plaintext readiness probe to the port of a management listener with management.transport = tls is refused
/// without an HTTP answer
/// Clause: 6.2
/// Recorded: the management listener is Andromeda's own and shares the broker's TLS identity; with its transport set
/// to tls it serves no plaintext
#[gtest]
#[tokio::test]
async fn a_tls_tls_0012_a_plaintext_request_to_the_tls_management_listener_gets_no_http_answer() -> Result<()> {
    let plaintext = broker::client(BrokerRole::TlsManagementPlaintext)?;
    // The readiness probe sits at the root of the management listener, outside the API root.
    let probe = SuiteConfig::get()?.broker(BrokerRole::TlsManagementPlaintext)?.join("readyz")?;
    let answer = plaintext
        .send_unchecked(request_to(&plaintext, Method::GET, "")?.uri(probe.as_str().parse()?))
        .await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
