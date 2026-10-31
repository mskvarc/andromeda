//! A 24 kib query is served over http11 and http2.

use andromeda_test_suite::{
    config::BrokerRole,
    namespace::Namespace,
    problem::answered,
    tls_connection::{Connection, HttpVersion},
};
use googletest::prelude::*;
use http::{StatusCode, Version};
use ngsi_ld::{http::headers::Accept, query::q::Query};

/// Pins: Query Entities whose q carries a 24 `KiB` string, under the 32 `KiB` request header section limit, answers
/// 200 with no entity over HTTP/1.1 and over HTTP/2 alike
/// Clause: 5.7.2, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 defers to the HTTP binding's errors without fixing a header
/// size; the stack runs the default `server.max_request_header_section` of 32 `KiB`, measured as RFC 9113 section
/// 6.5.2 measures it, and RFC 6585 section 5 answers a section over it 431
#[gtest]
#[tokio::test]
async fn a_lim_hdr_9001_a_24_kib_query_is_served_over_http11_and_http2() -> Result<()> {
    let namespace = Namespace::fresh();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for (offered, expected) in [(HttpVersion::Http11, Version::HTTP_11), (HttpVersion::Http2, Version::HTTP_2)] {
        let broker_tls = Connection::new().trusting("tls/ca.crt").offering(offered).client(BrokerRole::Tls).await?;
        let answer = broker_tls
            .with_heads()
            .with_accept(Accept::any())
            .entities()
            .query()
            .entity_type(&vehicle_type)
            .q(format!(r#"name=="{}""#, "a".repeat(24576)).parse::<Query>()?)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::OK));
        let headed = answer.as_ref().or_fail()?;
        expect_that!(headed.head.version(), eq(expected));
        expect_that!(headed.outcome.items(), is_empty());
    }
    Ok(())
}
