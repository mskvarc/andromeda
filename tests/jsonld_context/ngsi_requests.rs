//! The NGSI-LD requests the cases make besides the `/jsonldContexts` ones.

use crate::{broker_endpoints::broker_url, context_links::context_link, user_contexts::DEVICE_IRI};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

/// Create an Entity from an `application/ld+json` body: the status and the
/// `Location` header.
pub async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .map(|value| value.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// Create a `Device` `id` carrying `speed`, from an `application/json` body
/// read under the one context `link` names, asserting it is created.
pub async fn create_device(client: &Client, id: &str, link: &str) {
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(link))
        .json(&json!({ "id": id, "type": "Device", "speed": { "type": "Property", "value": 3 } }))
        .send()
        .await
        .expect("create request failed");
    assert_eq!(response.status(), StatusCode::CREATED, "the device is created: {:?}", response.text().await);
}

/// The query path that finds `Device` `id` whatever context the query names:
/// the type is given as an absolute IRI, which IRI expansion leaves as it is.
pub fn device_query(id: &str) -> String {
    format!("entities?type={}&id={id}", urlencoding::encode(DEVICE_IRI))
}

/// Delete Entity `id`, ignoring the outcome.
pub async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(broker_url(&format!("entities/{id}"))).send().await;
}

/// GET `path` negotiated as `accept`, naming `link` as the request's context
/// when given.
pub async fn get(client: &Client, path: &str, accept: &str, link: Option<&str>) -> Response {
    let mut request = client.get(broker_url(path)).header("Accept", accept);
    if let Some(link) = link {
        request = request.header("Link", context_link(link));
    }
    request.send().await.expect("GET request failed")
}

/// The JSON body of a 200 answer to `response`.
pub async fn ok_body(response: Response) -> Value {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Entity `id` as stored: read back without any context, so every term the
/// core `@context` does not define appears under its full IRI.
pub async fn stored_entity(client: &Client, id: &str) -> Value {
    ok_body(get(client, &format!("entities/{id}"), "application/json", None).await).await
}

/// Create a Subscription from an `application/ld+json` body: the status and
/// the `Location` header.
pub async fn create_subscription(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let response = client
        .post(broker_url("subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_subscription request failed");
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .map(|value| value.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// Delete Subscription `id`, ignoring the outcome.
pub async fn delete_subscription(client: &Client, id: &str) {
    let _ = client.delete(broker_url(&format!("subscriptions/{id}"))).send().await;
}

/// The `jsonldContext` Subscription `id` notifies under.
pub async fn jsonld_context_of(client: &Client, id: &str) -> String {
    let subscription = ok_body(get(client, &format!("subscriptions/{id}"), "application/json", None).await).await;
    subscription["jsonldContext"]
        .as_str()
        .expect("the subscription carries a jsonldContext")
        .to_owned()
}
