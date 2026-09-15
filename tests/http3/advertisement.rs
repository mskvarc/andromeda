//! The TCP listener advertises the HTTP/3 endpoint on the same port via `Alt-Svc`.

use crate::client::tcp_base;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

#[tokio::test]
async fn tcp_responses_advertise_http3_via_alt_svc() {
    let url = format!("{}/health", tcp_base());
    let response = reqwest::get(&url).await.expect("the broker answers over TCP");
    assert_eq!(response.status(), StatusCode::OK, "GET /health over TCP should be 200");

    let alt_svc = response
        .headers()
        .get("alt-svc")
        .expect("a TCP response advertises HTTP/3 through Alt-Svc while the QUIC listener is up")
        .to_str()
        .expect("the Alt-Svc value is ASCII");

    // RFC 7838: h3 on the same port, empty host meaning "this origin".
    assert!(alt_svc.contains("h3="), "Alt-Svc should advertise the h3 protocol: {alt_svc}");
    assert!(alt_svc.contains(":1056"), "Alt-Svc should advertise the shared port 1056: {alt_svc}");
}
