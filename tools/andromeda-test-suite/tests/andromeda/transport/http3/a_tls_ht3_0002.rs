//! Health is served over HTTP/3.

use andromeda_test_suite::{
    config::BrokerRole,
    health_check::{HealthDocument, HealthType},
    tls_connection::{Connection, HttpVersion},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, Version};

/// Pins: GET /ngsi-ld/v1/health from a client speaking only HTTP/3 is answered over HTTP/3 with 200 and a
/// `HealthCheck` document
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener; the health endpoint is an Andromeda extension
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0002_health_is_served_over_http3() -> Result<()> {
    let quic = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    // The health report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let answer = quic.send_unchecked(request_to(&quic, Method::GET, "health")?).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(answer.version(), eq(Version::HTTP_3));
    let health: HealthDocument = serde_json::from_slice(answer.body())?;
    expect_that!(health.health_type, eq(HealthType::HealthCheck));
    Ok(())
}
