//! The reverse-proxy model: nginx terminates TLS in front of a plaintext broker.

use crate::stack::{Endpoint, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, header::LOCATION, redirect::Policy, tls};
use serde_json::Value;

#[tokio::test]
async fn the_terminator_serves_the_broker_behind_it_over_https() {
    let response = trusting_the_stack()
        .build()
        .expect("client builds")
        .get(Endpoint::Proxy.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the terminator answers over TLS");
    assert_eq!(response.status(), StatusCode::OK);
    let identity: Value = response.json().await.expect("the identity is JSON");
    assert_eq!(identity["contextSourceAlias"], "tls-broker");
}

/// Clause 6.2 names TLS 1.2 as the floor the terminator has to offer.
#[tokio::test]
async fn the_terminator_serves_a_tls12_only_client() {
    let response = trusting_the_stack()
        .tls_version_max(tls::Version::TLS_1_2)
        .build()
        .expect("client builds")
        .get(Endpoint::Proxy.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the terminator answers over TLS 1.2");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Clause 6.2: "HTTP without TLS is not recommended." The terminator sends a
/// plaintext client to its TLS listener rather than serving it.
#[tokio::test]
async fn the_terminator_redirects_plaintext_clients_to_its_tls_listener() {
    let response = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("client builds")
        .get(Endpoint::ProxyPlaintext.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the plaintext listener answers");
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect names its target");
    assert!(location.starts_with("https://"), "{location}");
    assert!(location.ends_with("/ngsi-ld/v1/info/sourceIdentity"), "{location}");
}
