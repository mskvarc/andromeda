//! The terminator serves the plaintext broker behind it.

use andromeda_test_suite::{config::BrokerRole, tls_connection::Connection};
use googletest::prelude::*;
use http::{StatusCode, Version};
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: a request over TLS to the terminator is answered by the plaintext broker behind it, which names its
/// configured identity, over the HTTP/2 the terminator negotiates
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker serves plaintext behind a TLS terminator and believes the forwarded fields the terminator sets
#[gtest]
#[tokio::test]
async fn a_tls_trm_0001_the_terminator_serves_the_plaintext_broker_behind_it() -> Result<()> {
    let terminator = Connection::new().trusting("tls/ca.crt").client(BrokerRole::Terminator).await?;
    let headed = terminator.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(headed.head.version(), eq(Version::HTTP_2));
    expect_that!(&headed.outcome.alias, eq(&"broker-edge".parse::<ContextSourceAlias>()?));
    Ok(())
}
