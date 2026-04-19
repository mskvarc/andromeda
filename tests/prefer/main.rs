//! `Prefer` / `Preference-Applied` header integration tests (NGSI-LD 6.3.6).
//!
//! Naming convention: `prefer_<area>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn get_entity(client: &NgsiLdClient, id: &str, accept: &str, prefer: Option<&str>, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let path = format!("/ngsi-ld/v1/entities/{}{sep}{query}", urlencoding::encode(id));
    let mut request = client
        .raw(Method::GET, &path)
        .header("Accept", accept)
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT));
    if let Some(prefer) = prefer {
        request = request.header("Prefer", prefer);
    }
    let response = request.send().await.expect("get_entity failed");
    (response.status, response.headers, response.body)
}

async fn query_entities(client: &NgsiLdClient, accept: &str, prefer: Option<&str>, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let path = format!("/ngsi-ld/v1/entities{sep}{query}");
    let mut request = client
        .raw(Method::GET, &path)
        .header("Accept", accept)
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT));
    if let Some(prefer) = prefer {
        request = request.header("Prefer", prefer);
    }
    let response = request.send().await.expect("query_entities failed");
    (response.status, response.headers, response.body)
}

fn building_point(id: &str, coords: [f64; 2]) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": coords}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn building_temp(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 25.0},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

#[tokio::test]
async fn prefer_geojson_retrieve_default_body_includes_context_inline() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer1");
    create(&client, &building_point(&id, [13.3986, 52.5547])).await;
    let (status, _, body) = get_entity(&client, &id, "application/geo+json", None, "geometryProperty=location").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some(), "default GeoJSON response must include @context inline");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_geojson_retrieve_body_ld_json_includes_context_inline() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer2");
    create(&client, &building_point(&id, [13.3986, 52.5547])).await;
    let (status, _, body) = get_entity(&client, &id, "application/geo+json", Some("body=ld+json"), "geometryProperty=location").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some(), "Prefer: body=ld+json must include @context inline");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_geojson_retrieve_body_json_moves_context_to_link_header() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer3");
    create(&client, &building_point(&id, [13.3986, 52.5547])).await;
    let (status, headers, body) = get_entity(&client, &id, "application/geo+json", Some("body=json"), "geometryProperty=location").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_none(), "Prefer: body=json must omit @context from body");
    assert!(headers.get("link").is_some(), "Prefer: body=json must move @context to Link header");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_geojson_query_body_json_removes_context_from_collection_body() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer4");
    create(&client, &building_point(&id, [2.3522, 48.8566])).await;

    let (status, headers, body) = query_entities(
        &client,
        "application/geo+json",
        Some("body=json"),
        &format!("type=Building&id={}", urlencoding::encode(&id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "FeatureCollection");
    assert!(body.get("@context").is_none());
    assert!(headers.get("link").is_some());
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_version_retrieve_older_version_returns_non_authoritative_with_preference_applied() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer5");
    create(&client, &building_temp(&id)).await;

    let (status, headers, _) = get_entity(&client, &id, "application/ld+json", Some("ngsi-ld=1.3"), "").await;
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    let pref = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    assert_eq!(pref, Some("ngsi-ld=1.9"));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_version_query_older_version_returns_non_authoritative_with_preference_applied() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer6");
    create(&client, &building_temp(&id)).await;

    let (status, headers, _) = query_entities(
        &client,
        "application/ld+json",
        Some("ngsi-ld=1.4"),
        &format!("type=Building&id={}", urlencoding::encode(&id)),
    )
    .await;
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    let pref = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    assert_eq!(pref, Some("ngsi-ld=1.9"));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_absent_retrieve_returns_ok_without_preference_applied_header() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer7");
    create(&client, &building_temp(&id)).await;

    let (status, headers, _) = get_entity(&client, &id, "application/ld+json", None, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("preference-applied").is_none());
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_combined_body_json_and_version_returns_non_authoritative_without_context_in_body() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer8");
    create(&client, &building_point(&id, [13.3986, 52.5547])).await;

    let (status, headers, body) = get_entity(
        &client,
        &id,
        "application/geo+json",
        Some("body=json, ngsi-ld=1.3"),
        "geometryProperty=location",
    )
    .await;
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    assert!(body.get("@context").is_none());
    assert!(headers.get("link").is_some());
    let pref = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    assert_eq!(pref, Some("ngsi-ld=1.9"));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_version_1_0_retrieve_removes_dataset_id_from_attribute() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer9");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ngsi-ld:Dataset:temp1"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let (status, _, body) = get_entity(&client, &id, "application/ld+json", Some("ngsi-ld=1.0"), "").await;
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    let temp_key = "https://uri.fiware.org/ns/data-models#temperature";
    let temp = &body[temp_key];
    assert!(temp.get("datasetId").is_none(), "v1.0 adaptation must strip datasetId");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn prefer_version_1_0_retrieve_removes_scope_from_entity() {
    let scope = TestScope::new("prefer");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:prefer10");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "scope": ["/A/B"],
            "temperature": {"type": "Property", "value": 25.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let (status, _, body) = get_entity(&client, &id, "application/ld+json", Some("ngsi-ld=1.0"), "").await;
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    assert!(body.get("scope").is_none(), "v1.0 adaptation must strip scope field");
    scope.cleanup().await.expect("cleanup failed");
}
