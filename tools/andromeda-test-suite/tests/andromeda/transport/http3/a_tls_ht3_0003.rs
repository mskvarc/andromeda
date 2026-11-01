//! A JSON body is returned over HTTP/3.

use andromeda_test_suite::{
    config::BrokerRole,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use http::{StatusCode, Version};
use ngsi_ld::{http::headers::Accept, model::ContextSourceAlias};

/// Pins: Retrieve Source Identity from a client speaking only HTTP/3 is answered over HTTP/3 with the broker's
/// identity document, which names its configured alias
/// Clause: 5.15.2, 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0003_a_json_body_is_returned_over_http3() -> Result<()> {
    let quic = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    let headed = quic.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(headed.head.version(), eq(Version::HTTP_3));
    expect_that!(&headed.outcome.alias, eq(&"broker-tls".parse::<ContextSourceAlias>()?));
    Ok(())
}
