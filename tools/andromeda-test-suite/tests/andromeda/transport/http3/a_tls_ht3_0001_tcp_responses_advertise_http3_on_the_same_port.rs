//! A tls ht3 0001 tcp responses advertise http3 on the same port.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    tls_connection::Connection,
    wire::header_values,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: while the HTTP/3 listener is up, a response over TCP carries Alt-Svc advertising h3 on the same port
/// number with an empty host, meaning this origin (RFC 7838 clause 3)
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0001_tcp_responses_advertise_http3_on_the_same_port() -> Result<()> {
    let origin = SuiteConfig::get()?.broker(BrokerRole::Tls)?;
    let port = origin.port_or_known_default().or_fail()?;
    let broker_tls = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let headed = broker_tls.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    // An empty host in the alternative means this origin (IETF RFC 7838 section 3).
    let advertised = header_values(headed.head.headers(), &HeaderName::from_static("alt-svc"));
    expect_that!(advertised, contains(contains_substring(format!("h3=\":{port}\""))));
    Ok(())
}
