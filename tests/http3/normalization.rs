//! Trailing-slash normalization applies over HTTP/3 exactly as over TCP.

use crate::client::{Http3Client, wait_until_ready};
use http::{Method, StatusCode};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_trailing_slash_is_normalized_over_http3() {
    wait_until_ready().await;
    let client = Http3Client::connect_trusted().await;
    // The route is registered without a trailing slash; the broker trims it
    // before routing on the QUIC path the same way it does on TCP.
    let response = client.request(Method::GET, "/ngsi-ld/v1/health/", &[], &[]).await;
    client.close().await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "a trailing slash should be normalized to reach /health over HTTP/3"
    );
}
