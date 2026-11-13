//! Andromeda's own routes are served over HTTP/3.

use andromeda_test_suite::{
    component_report::{ComponentReportList, ReportType},
    config::BrokerRole,
    tls_connection::{Connection, HttpVersion},
    wire::andromeda_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, Version};

/// Pins: GET /ngsi-ld/andromeda/v1/version from a client speaking only HTTP/3 is answered over HTTP/3 with 200 and a
/// `ComponentReportList` document
/// Clause: 6.2
/// Recorded: HTTP/3 is Andromeda's; clause 6.2 names HTTP over TLS without fixing the HTTP version. The TLS broker
/// serves HTTP/3 over QUIC on the port number of its TCP listener; the version endpoint is an Andromeda extension
#[gtest]
#[tokio::test]
async fn a_tls_ht3_0002_andromedas_own_routes_are_served_over_http3() -> Result<()> {
    let quic = Connection::new()
        .trusting("tls/ca.crt")
        .offering(HttpVersion::Http3)
        .client(BrokerRole::Tls)
        .await?;
    // The component report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let answer = quic.send_unchecked(andromeda_request_to(&quic, Method::GET, "version")?).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(answer.version(), eq(Version::HTTP_3));
    let report: ComponentReportList = serde_json::from_slice(answer.body())?;
    expect_that!(report.report_type, eq(ReportType::ComponentReportList));
    Ok(())
}
