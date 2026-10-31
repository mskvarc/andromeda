//! Responses carry strict transport security.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection, wire::header_value};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: a response the broker sends over its own TLS listener carries Strict-Transport-Security, which RFC 6797
/// clause 8.1 has a client act on only when it arrives over a secure transport
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker terminates TLS itself (server.transport = tls); Andromeda adds Strict-Transport-Security to responses on
/// a TLS listener
#[gtest]
#[tokio::test]
async fn a_tls_tls_0004_responses_carry_strict_transport_security() -> Result<()> {
    let broker_tls = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Tls).await?;
    let headed = broker_tls.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(&headed.outcome.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    expect_that!(
        header_value(headed.head.headers(), &HeaderName::from_static("strict-transport-security")),
        some(anything())
    );
    Ok(())
}
