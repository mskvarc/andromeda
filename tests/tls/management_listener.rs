//! A `separate-port` management listener with `management.transport = "tls"`
//! presents the shared identity too.

use crate::stack::{Endpoint, certificate_der, presented_leaf, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};

#[tokio::test]
async fn the_management_listener_answers_readiness_over_tls() {
    let response = trusting_the_stack()
        .build()
        .expect("client builds")
        .get(Endpoint::Management.url("readyz"))
        .send()
        .await
        .expect("the management listener answers over TLS");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        presented_leaf(&response),
        Some(certificate_der("broker.crt")),
        "the shared identity is presented"
    );
}

#[tokio::test]
async fn the_management_listener_refuses_plaintext() {
    let plaintext = Endpoint::Management.url("livez").replacen("https://", "http://", 1);
    let outcome = Client::new().get(plaintext).send().await;
    assert!(outcome.is_err(), "a plaintext probe gets no HTTP response: {outcome:?}");
}
