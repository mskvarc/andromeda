//! A plaintext request to the TLS listener gets no HTTP answer.

use andromeda_test_suite::{broker, config::BrokerRole, connection_outcome::ConnectionOutcome};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: a plaintext HTTP request to the port of the broker's own TLS listener is refused without an HTTP answer;
/// the listener serves no plaintext beside TLS
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); clause 6.2 does not recommend HTTP without TLS, and a TLS listener does not serve it
#[gtest]
#[tokio::test]
async fn a_tls_tls_0011_a_plaintext_request_to_the_tls_listener_gets_no_http_answer() -> Result<()> {
    let plaintext = broker::client(BrokerRole::TlsPlaintext)?;
    let answer = plaintext.with_accept(Accept::any()).source_identity().send().await;
    expect_that!(ConnectionOutcome::of_request(&answer), eq(ConnectionOutcome::Refused));
    Ok(())
}
