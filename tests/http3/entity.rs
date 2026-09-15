//! An entity round-trips over HTTP/3, and is visible over TCP: both transports
//! carry request and response bodies to and from the one broker store.

use crate::client::{Http3Client, tcp_base, wait_until_ready};
use http::{Method, StatusCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use uuid::Uuid;

/// The ETSI core context the broker resolves from its built-in set, so the
/// create needs no external context fetch.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// A unique entity document and its identifier.
fn unique_entity() -> (String, Value) {
    let id = format!("urn:ngsi-ld:Http3TestEntity:{}", Uuid::new_v4());
    let entity = json!({
        "id": id,
        "type": "Http3TestEntity",
        "speed": { "type": "Property", "value": 42 },
        "@context": CORE_CONTEXT,
    });
    (id, entity)
}

#[tokio::test]
async fn an_entity_created_over_http3_is_retrievable_over_http3() {
    wait_until_ready().await;
    let client = Http3Client::connect_trusted().await;
    let (id, entity) = unique_entity();
    let body = serde_json::to_vec(&entity).expect("entity serializes");

    let created = client
        .request(Method::POST, "/ngsi-ld/v1/entities", &[("content-type", "application/ld+json")], &body)
        .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "POST /entities over HTTP/3 should be 201: {}",
        created.text()
    );

    let retrieved = client
        .request(Method::GET, &format!("/ngsi-ld/v1/entities/{id}"), &[("accept", "application/ld+json")], &[])
        .await;
    client.close().await;
    assert_eq!(retrieved.status, StatusCode::OK, "GET /entities/{{id}} over HTTP/3 should be 200");
    let document = retrieved.json();
    assert_eq!(document["id"], id, "the retrieved entity should carry the created id");
    assert_eq!(document["type"], "Http3TestEntity", "the retrieved entity should carry its type");
    assert_eq!(document["speed"]["value"], 42, "the retrieved entity should carry its property value");
}

#[tokio::test]
async fn an_entity_created_over_http3_is_visible_over_tcp() {
    wait_until_ready().await;
    let client = Http3Client::connect_trusted().await;
    let (id, entity) = unique_entity();
    let body = serde_json::to_vec(&entity).expect("entity serializes");

    let created = client
        .request(Method::POST, "/ngsi-ld/v1/entities", &[("content-type", "application/ld+json")], &body)
        .await;
    client.close().await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "POST /entities over HTTP/3 should be 201: {}",
        created.text()
    );

    // The same entity must read back over the TCP listener, proving both
    // transports serve the one broker store rather than two isolated ones.
    let url = format!("{}/entities/{id}", tcp_base());
    let response = reqwest::Client::new()
        .get(&url)
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("the broker answers over TCP");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "the entity created over HTTP/3 should read back over TCP"
    );
    let document: Value = response.json().await.expect("the TCP response is JSON");
    assert_eq!(document["id"], id, "the entity read over TCP should carry the id created over HTTP/3");
}
