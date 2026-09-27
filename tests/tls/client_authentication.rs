//! `tls.client_authentication` decides which clients complete the handshake.

use crate::stack::{ClientCertificate, Endpoint, presenting, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, tls};

/// The status `client` gets for the identity of `endpoint`, or `None` when the
/// connection is refused.
async fn status_at(client: &Client, endpoint: Endpoint) -> Option<StatusCode> {
    client
        .get(endpoint.url("info/sourceIdentity"))
        .send()
        .await
        .ok()
        .map(|response| response.status())
}

#[tokio::test]
async fn optional_client_authentication_serves_a_client_without_a_certificate() {
    let anonymous = trusting_the_stack().build().expect("client builds");
    assert_eq!(status_at(&anonymous, Endpoint::Native).await, Some(StatusCode::OK));
}

#[tokio::test]
async fn optional_client_authentication_serves_a_client_certificate_from_the_trusted_authority() {
    let authorized = presenting(trusting_the_stack(), ClientCertificate::Authorized);
    assert_eq!(status_at(&authorized, Endpoint::Native).await, Some(StatusCode::OK));
}

#[tokio::test]
async fn optional_client_authentication_refuses_a_client_certificate_from_an_unknown_issuer() {
    let stranger = presenting(trusting_the_stack(), ClientCertificate::Stranger);
    assert_eq!(status_at(&stranger, Endpoint::Native).await, None);
}

#[tokio::test]
async fn required_client_authentication_refuses_a_client_without_a_certificate() {
    let anonymous = trusting_the_stack().build().expect("client builds");
    assert_eq!(status_at(&anonymous, Endpoint::Mutual).await, None);
}

#[tokio::test]
async fn required_client_authentication_serves_a_client_certificate_from_the_trusted_authority() {
    let authorized = presenting(trusting_the_stack(), ClientCertificate::Authorized);
    assert_eq!(status_at(&authorized, Endpoint::Mutual).await, Some(StatusCode::OK));
}

#[tokio::test]
async fn required_client_authentication_refuses_a_client_certificate_from_an_unknown_issuer() {
    let stranger = presenting(trusting_the_stack(), ClientCertificate::Stranger);
    assert_eq!(status_at(&stranger, Endpoint::Mutual).await, None);
}

/// The mutual broker is configured with `minimum_protocol_version = "tls1.3"`,
/// so even an authorized client is refused over TLS 1.2.
#[tokio::test]
async fn a_tls13_floor_refuses_an_authorized_tls12_only_client() {
    let tls12 = presenting(trusting_the_stack().tls_version_max(tls::Version::TLS_1_2), ClientCertificate::Authorized);
    assert_eq!(status_at(&tls12, Endpoint::Mutual).await, None);
}
