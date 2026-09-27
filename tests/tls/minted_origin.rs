//! Clause 6.2 makes the `apiRoot` "include the scheme (`http` or `https`),
//! host and optional port", and clause 5.13 has the broker store the URL it
//! serves a hosted `@context` at. Both models must mint that URL with the
//! `https` origin the client used, and the URL must resolve.

use crate::stack::{Endpoint, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, header::LOCATION};
use serde_json::{Value, json};
use uuid::Uuid;

/// Host a fresh `@context` at `endpoint` with `client`, sending `fields`, and
/// return the URL the broker stored for it after checking it resolves; the
/// context is deleted again.
async fn hosted_context_url(client: &Client, endpoint: Endpoint, fields: &[(&str, &str)]) -> String {
    let term = format!("tlsSuite{}", Uuid::new_v4().simple());
    let mut request = client
        .post(endpoint.url("jsonldContexts"))
        .json(&json!({ "@context": { term: "https://example.org/tls-suite" } }));
    for (name, value) in fields {
        request = request.header(*name, *value);
    }
    let created = request.send().await.expect("the context is hosted");
    assert_eq!(created.status(), StatusCode::CREATED, "{:?}", created.text().await);
    let location = created
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a hosted context has a location")
        .to_owned();
    let identifier = location.rsplit('/').next().expect("the location ends in the identifier").to_owned();

    let listing: Value = client
        .get(endpoint.url("jsonldContexts?details=true&kind=Hosted"))
        .send()
        .await
        .expect("the contexts are listed")
        .json()
        .await
        .expect("the listing is JSON");
    let url = listing
        .as_array()
        .expect("the listing is an array")
        .iter()
        .find(|context| context["localId"] == identifier.as_str())
        .and_then(|context| context["URL"].as_str())
        .unwrap_or_else(|| panic!("the listing carries a URL for {identifier}: {listing}"))
        .to_owned();

    let served = client.get(&url).send().await.expect("the stored URL resolves");
    assert_eq!(served.status(), StatusCode::OK, "{url} serves the context");

    let deleted = client
        .delete(endpoint.url(&format!("jsonldContexts/{identifier}")))
        .send()
        .await
        .expect("the context is deleted");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    url
}

#[tokio::test]
async fn a_context_hosted_over_native_http2_is_stored_under_an_https_url() {
    let client = trusting_the_stack().build().expect("client builds");
    let url = hosted_context_url(&client, Endpoint::Native, &[]).await;
    assert!(url.starts_with(&format!("{}/ngsi-ld/v1/jsonldContexts/", Endpoint::Native.origin())), "{url}");
}

#[tokio::test]
async fn a_context_hosted_over_native_http11_is_stored_under_an_https_url() {
    let client = trusting_the_stack().http1_only().build().expect("client builds");
    let url = hosted_context_url(&client, Endpoint::Native, &[]).await;
    assert!(url.starts_with(&format!("{}/ngsi-ld/v1/jsonldContexts/", Endpoint::Native.origin())), "{url}");
}

/// The native broker trusts no forwarded field, so a client claiming plaintext
/// cannot downgrade the scheme of a URL minted on a TLS listener.
#[tokio::test]
async fn a_client_claiming_plaintext_cannot_downgrade_a_native_tls_url() {
    let client = trusting_the_stack().build().expect("client builds");
    let url = hosted_context_url(&client, Endpoint::Native, &[("x-forwarded-proto", "http"), ("forwarded", "proto=http")]).await;
    assert!(url.starts_with("https://"), "{url}");
}

#[tokio::test]
async fn a_context_hosted_through_the_terminator_is_stored_under_its_https_origin() {
    let client = trusting_the_stack().build().expect("client builds");
    let url = hosted_context_url(&client, Endpoint::Proxy, &[]).await;
    assert!(url.starts_with(&format!("{}/ngsi-ld/v1/jsonldContexts/", Endpoint::Proxy.origin())), "{url}");
}

/// The terminator replaces the forwarded fields rather than appending to them,
/// so what the client sent never reaches the broker that trusts them.
#[tokio::test]
async fn the_terminator_replaces_a_client_forwarded_scheme() {
    let client = trusting_the_stack().build().expect("client builds");
    let url = hosted_context_url(&client, Endpoint::Proxy, &[("x-forwarded-proto", "http"), ("forwarded", "proto=http")]).await;
    assert!(url.starts_with(&format!("{}/", Endpoint::Proxy.origin())), "{url}");
}
