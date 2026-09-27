//! A broker with `server.transport = "tls"` serves HTTPS itself, meeting clause
//! 6.2 with no proxy in front of it.

use crate::stack::{Endpoint, certificate_der, presented_leaf, trusting, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, Version, tls};
use serde_json::Value;

/// The identity the native broker answers with.
async fn source_alias(client: &Client) -> String {
    let response = client
        .get(Endpoint::Native.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the native broker answers over TLS");
    assert_eq!(response.status(), StatusCode::OK);
    let identity: Value = response.json().await.expect("the identity is JSON");
    identity["contextSourceAlias"].as_str().expect("the identity names its alias").to_owned()
}

#[tokio::test]
async fn the_native_listener_serves_https_over_http2() {
    let client = trusting_the_stack().build().expect("client builds");
    let response = client
        .get(Endpoint::Native.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the native broker answers over TLS");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.version(), Version::HTTP_2, "ALPN negotiates h2 first");
    assert_eq!(source_alias(&client).await, "tls-native-broker");
}

#[tokio::test]
async fn an_http11_only_client_is_served() {
    let client = trusting_the_stack().http1_only().build().expect("client builds");
    let response = client
        .get(Endpoint::Native.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the native broker answers HTTP/1.1 over TLS");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.version(), Version::HTTP_11);
}

/// Clause 6.2: "TLS version 1.2 as defined by IETF RFC 5246 shall be supported."
#[tokio::test]
async fn a_tls12_only_client_is_served_under_the_default_floor() {
    let client = trusting_the_stack().tls_version_max(tls::Version::TLS_1_2).build().expect("client builds");
    assert_eq!(source_alias(&client).await, "tls-native-broker");
}

#[tokio::test]
async fn the_presented_leaf_is_the_configured_certificate() {
    let response = trusting_the_stack()
        .build()
        .expect("client builds")
        .get(Endpoint::Native.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the native broker answers over TLS");
    assert_eq!(presented_leaf(&response), Some(certificate_der("broker.crt")));
}

/// A TLS listener serves no plaintext on the same port.
#[tokio::test]
async fn a_plaintext_request_is_refused() {
    let plaintext = Endpoint::Native.url("info/sourceIdentity").replacen("https://", "http://", 1);
    let outcome = Client::new().get(plaintext).send().await;
    assert!(outcome.is_err(), "a plaintext request gets no HTTP response: {outcome:?}");
}

#[tokio::test]
async fn a_client_not_trusting_the_authority_is_refused() {
    let stranger = trusting("client-ca.crt").build().expect("client builds");
    let outcome = stranger.get(Endpoint::Native.url("info/sourceIdentity")).send().await;
    assert!(outcome.is_err(), "a certificate from an untrusted authority fails the handshake: {outcome:?}");
}

/// Over TLS the broker terminated itself, its `Strict-Transport-Security` field
/// arrives on a secure transport, so it is the policy a client acts on
/// (IETF RFC 6797 clause 8.1).
#[tokio::test]
async fn responses_carry_strict_transport_security() {
    let response = trusting_the_stack()
        .build()
        .expect("client builds")
        .get(Endpoint::Native.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the native broker answers over TLS");
    assert!(response.headers().contains_key("strict-transport-security"), "{:?}", response.headers());
}
