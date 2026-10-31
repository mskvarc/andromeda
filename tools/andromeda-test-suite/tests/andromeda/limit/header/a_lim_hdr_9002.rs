//! A 40 kib query is refused 431 over http11 and http2.

use andromeda_test_suite::{
    config::BrokerRole,
    namespace::Namespace,
    problem::answered,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::q::Query};

/// Pins: Query Entities whose q carries a 40 `KiB` string, over the 32 `KiB` request header section limit, answers
/// 431 without a body over HTTP/1.1 and over HTTP/2 alike
/// Clause: 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 defers to the HTTP binding's errors without fixing a header
/// size; the stack runs the default `server.max_request_header_section` of 32 `KiB`, measured as RFC 9113 section
/// 6.5.2 measures it, and RFC 6585 section 5 answers a section over it 431
#[gtest]
#[tokio::test]
async fn a_lim_hdr_9002_a_40_kib_query_is_refused_431_over_http11_and_http2() -> Result<()> {
    let namespace = Namespace::fresh();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_tls_2 = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http11)
        .client(BrokerRole::Tls)
        .await?;
    let broker_tls_connection = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http2)
        .client(BrokerRole::Tls)
        .await?;
    let answer = broker_tls_2
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(format!(r#"name=="{}""#, "a".repeat(40960)).parse::<Query>()?)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE).without_body());
    let answer_2 = broker_tls_connection
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(format!(r#"name=="{}""#, "a".repeat(40960)).parse::<Query>()?)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE).without_body());
    Ok(())
}
