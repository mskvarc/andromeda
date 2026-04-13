//! Common behaviour integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Naming convention: `<category>_<scenario>_<expected_outcome>`
//!
//! All tests assume the broker is running at `BASE_URL` with a clean-enough state
//! (each test creates and cleans up its own resources).

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const UNRETRIEVABLE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-non-existing.jsonld";
const INVALID_REQUEST_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
const LD_CONTEXT_NOT_AVAILABLE_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

/// POST /entities with JSON-LD body. Returns status and Location header.
async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp
        .headers()
        .get("location")
        .map(|v| v.to_str().expect("non-ascii location header").to_owned());
    (status, location)
}

/// GET /entities/{id} with application/ld+json accept.
async fn get_entity(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entities/{id}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// DELETE /entities/{id}.
async fn delete_entity(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/entities/{id}"))
        .send()
        .await
        .expect("delete_entity request failed")
        .status()
}

/// POST /subscriptions with JSON-LD body.
async fn create_subscription(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_subscription request failed")
        .status()
}

/// DELETE /subscriptions/{id}.
async fn delete_subscription(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/subscriptions/{id}"))
        .send()
        .await
        .expect("delete_subscription request failed")
        .status()
}

/// POST /temporal/entities with JSON-LD body.
async fn create_temporal_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_temporal_entity request failed")
        .status()
}

/// DELETE /temporal/entities/{id}.
async fn delete_temporal_entity(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/temporal/entities/{id}"))
        .send()
        .await
        .expect("delete_temporal_entity request failed")
        .status()
}

/// POST /csourceRegistrations with JSON-LD body.
async fn create_registration(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_registration request failed")
        .status()
}

/// DELETE /csourceRegistrations/{id}.
async fn delete_registration(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/csourceRegistrations/{id}"))
        .send()
        .await
        .expect("delete_registration request failed")
        .status()
}

/// POST /entityOperations/create with JSON-LD body.
async fn batch_create(client: &Client, body: &Value) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("batch_create request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Generic request returning status + body.
async fn raw_request(
    client: &Client,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    content_type: Option<&str>,
    accept: Option<&str>,
) -> (StatusCode, reqwest::header::HeaderMap, Value) {
    let url = format!("{BASE_URL}{path}");
    let mut builder = client.request(method, &url);
    if let Some(ct) = content_type {
        builder = builder.header("Content-Type", ct);
    }
    if let Some(acc) = accept {
        builder = builder.header("Accept", acc);
    }
    if let Some(b) = body {
        builder = builder.body(b.to_vec());
    }
    let resp = builder.send().await.expect("raw_request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

// ---------------------------------------------------------------------------
// Smoke: entity lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn smoke_create_retrieve_delete_full_lifecycle() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:smoke");
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "@context": CORE_CONTEXT
    });

    // Create
    let (status, location) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create should return 201");
    assert!(location.is_some(), "create should return Location header");

    // Retrieve
    let (status, body) = get_entity(&c, &id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "get should return 200");
    pretty_assertions::assert_eq!(body["id"], id);
    pretty_assertions::assert_eq!(body["type"], "Vehicle");

    // Delete
    let status = delete_entity(&c, &id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "delete should return 204");

    // Retrieve after delete
    let (status, _) = get_entity(&c, &id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "get after delete should return 404");
}

// ---------------------------------------------------------------------------
// Invalid parameters: unrecognised query params must be rejected with 400
// NGSI-LD 6.3.5 -- all API operations shall reject unknown query parameters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_parameters_post_entities_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:inv-param-post");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "@context": CORE_CONTEXT
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/entities?invalidParam=invalidValue",
        Some(serde_json::to_vec(&entity).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_get_entities_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(&c, Method::GET, "/entities?type=Building&invalidParam=invalidValue", None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_post_temporal_entities_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:inv-param-temporal");
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"}
        ],
        "@context": CORE_CONTEXT
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/temporal/entities?invalidParam=invalidValue",
        Some(serde_json::to_vec(&entity).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_get_temporal_entities_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(&c, Method::GET, "/temporal/entities?type=Building&invalidParam=invalidValue", None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_get_types_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(&c, Method::GET, "/types?invalidParam=invalidValue", None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_post_subscriptions_returns_400() {
    let c = client();
    let sub_id = unique_id("urn:ngsi-ld:Subscription:inv-param-sub");
    let sub = json!({
        "id": &sub_id,
        "type": "Subscription",
        "timeInterval": 5,
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": CORE_CONTEXT
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/subscriptions?invalidParam=invalidValue",
        Some(serde_json::to_vec(&sub).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
async fn invalid_parameters_get_subscriptions_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(&c, Method::GET, "/subscriptions?invalidParam=invalidValue", None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

// ---------------------------------------------------------------------------
// Unsupported media type: Content-Type application/xml must be rejected with 415
// NGSI-LD 6.3.4
// ---------------------------------------------------------------------------

const XML_DUMMY_BODY: &[u8] = b"<entity/>";

#[tokio::test]
async fn unsupported_media_type_patch_entity_attr_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::PATCH,
        "/entities/urn:ngsi-ld:Vehicle:dummy/attrs/speed",
        Some(XML_DUMMY_BODY),
        Some("application/xml"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn unsupported_media_type_patch_subscription_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::PATCH,
        "/subscriptions/urn:ngsi-ld:Subscription:dummy",
        Some(XML_DUMMY_BODY),
        Some("application/xml"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn unsupported_media_type_post_entities_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::POST, "/entities", Some(XML_DUMMY_BODY), Some("application/xml"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn unsupported_media_type_post_subscriptions_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::POST, "/subscriptions", Some(XML_DUMMY_BODY), Some("application/xml"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn unsupported_media_type_post_batch_create_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::POST,
        "/entityOperations/create",
        Some(XML_DUMMY_BODY),
        Some("application/xml"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn unsupported_media_type_post_temporal_entities_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::POST, "/temporal/entities", Some(XML_DUMMY_BODY), Some("application/xml"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

// ---------------------------------------------------------------------------
// Not acceptable media type: Accept application/xml must be rejected with 406
// NGSI-LD 6.3.4
// ---------------------------------------------------------------------------

#[tokio::test]
async fn not_acceptable_get_entity_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/entities/urn:ngsi-ld:Building:dummy", None, None, Some("application/xml")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_subscription_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::GET,
        "/subscriptions/urn:ngsi-ld:Subscription:dummy",
        None,
        None,
        Some("application/xml"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_registrations_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/csourceRegistrations?type=Building", None, None, Some("application/xml")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_csource_subscriptions_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/csourceSubscriptions", None, None, Some("application/xml")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_temporal_entities_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::GET,
        "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
        None,
        None,
        Some("application/xml"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_subscription_geojson_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::GET,
        "/subscriptions/urn:ngsi-ld:Subscription:dummy",
        None,
        None,
        Some("application/geo+json"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_temporal_entities_geojson_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::GET,
        "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
        None,
        None,
        Some("application/geo+json"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn not_acceptable_get_registrations_geojson_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/csourceRegistrations?type=Building", None, None, Some("application/geo+json")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

// ---------------------------------------------------------------------------
// Merge-patch JSON: application/merge-patch+json accepted for partial updates
// NGSI-LD 6.3.3 / 5.6.3
// ---------------------------------------------------------------------------

#[tokio::test]
async fn merge_patch_partial_update_entity_attr_returns_204() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:merge-attr");
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": "Mercedes"},
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:Bob"}
        },
        "@context": CORE_CONTEXT
    });

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({"type": "Property", "value": "BMW"});
    let resp = c
        .patch(format!("{BASE_URL}/entities/{id}/attrs/brandName"))
        .header("Content-Type", "application/merge-patch+json")
        .json(&fragment)
        .send()
        .await
        .expect("patch request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Cleanup
    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn merge_patch_update_subscription_returns_204() {
    let c = client();
    let sub_id = unique_id("urn:ngsi-ld:Subscription:merge-sub");
    let sub = json!({
        "id": &sub_id,
        "type": "Subscription",
        "timeInterval": 5,
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": CORE_CONTEXT
    });

    let status = create_subscription(&c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let update = json!({
        "type": "Subscription",
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://my.second.endpoint.org/notify", "accept": "application/json"}
        }
    });
    let resp = c
        .patch(format!("{BASE_URL}/subscriptions/{sub_id}"))
        .header("Content-Type", "application/merge-patch+json")
        .json(&update)
        .send()
        .await
        .expect("patch subscription failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Cleanup
    delete_subscription(&c, &sub_id).await;
}

// ---------------------------------------------------------------------------
// LD context not available: unretrievable @context must yield 503
// NGSI-LD 6.3.2
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ld_context_not_available_create_entity_returns_503() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:ctx-unavail-entity");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "@context": [UNRETRIEVABLE_CONTEXT]
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/entities",
        Some(serde_json::to_vec(&entity).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE);
}

#[tokio::test]
async fn ld_context_not_available_create_subscription_returns_503() {
    let c = client();
    let sub_id = unique_id("urn:ngsi-ld:Subscription:ctx-unavail-sub");
    let sub = json!({
        "id": &sub_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": [UNRETRIEVABLE_CONTEXT]
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/subscriptions",
        Some(serde_json::to_vec(&sub).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE);
}

#[tokio::test]
async fn ld_context_not_available_create_temporal_entity_returns_503() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Bus:ctx-unavail-temporal");
    let entity = json!({
        "id": &id,
        "type": "Bus",
        "speed": [
            {"type": "Property", "value": 45, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 25, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:07:00Z"}
        ],
        "fuelLevel": [
            {"type": "Property", "value": 210, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 145, "observedAt": "2020-08-01T13:05:00Z"},
            {"type": "Property", "value": 124, "observedAt": "2020-08-01T14:07:00Z"}
        ],
        "@context": [UNRETRIEVABLE_CONTEXT]
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/temporal/entities",
        Some(serde_json::to_vec(&entity).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE);
}

#[tokio::test]
async fn ld_context_not_available_batch_create_returns_207_or_503() {
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:ctx-unavail-batch-a");
    let id2 = unique_id("urn:ngsi-ld:Building:ctx-unavail-batch-b");
    let entities = json!([
        {"id": &id1, "type": "Building", "@context": [UNRETRIEVABLE_CONTEXT]},
        {"id": &id2, "type": "Building", "@context": [UNRETRIEVABLE_CONTEXT]}
    ]);

    let (status, _body) = batch_create(&c, &entities).await;
    assert!(
        status == StatusCode::MULTI_STATUS || status == StatusCode::SERVICE_UNAVAILABLE,
        "expected 207 or 503, got {status}"
    );
}

#[tokio::test]
async fn ld_context_not_available_create_registration_returns_503() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:ctx-unavail-reg");
    let reg = json!({
        "id": &id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://my.csource.org:1026",
        "@context": [UNRETRIEVABLE_CONTEXT]
    });

    let (status, _, body) = raw_request(
        &c,
        Method::POST,
        "/csourceRegistrations",
        Some(serde_json::to_vec(&reg).expect("serialize").as_slice()),
        Some("application/ld+json"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE);
}

// ---------------------------------------------------------------------------
// GET without Accept (or Accept: */*): must return application/json + Link header
// NGSI-LD 6.3.4 clause 3
// ---------------------------------------------------------------------------

fn assert_json_content_type(headers: &reqwest::header::HeaderMap) {
    let ct = headers
        .get("content-type")
        .expect("should have Content-Type header")
        .to_str()
        .expect("non-ascii content-type");
    assert!(ct.contains("application/json"), "Content-Type should contain application/json, got {ct}");
}

fn assert_has_link_header(headers: &reqwest::header::HeaderMap) {
    assert!(headers.get("link").is_some(), "response should have Link header");
}

#[tokio::test]
async fn get_without_accept_entity_returns_json_with_link() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:no-accept-entity");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "@context": CORE_CONTEXT
    });

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = raw_request(&c, Method::GET, &format!("/entities/{id}"), None, None, Some("*/*")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_json_content_type(&headers);
    assert_has_link_header(&headers);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn get_without_accept_subscription_returns_json_with_link() {
    let c = client();
    let sub_id = unique_id("urn:ngsi-ld:Subscription:no-accept-sub");
    let sub = json!({
        "id": &sub_id,
        "type": "Subscription",
        "timeInterval": 5,
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": CORE_CONTEXT
    });

    let status = create_subscription(&c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = raw_request(&c, Method::GET, &format!("/subscriptions/{sub_id}"), None, None, Some("*/*")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_json_content_type(&headers);
    assert_has_link_header(&headers);

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
async fn get_without_accept_registrations_returns_json_with_link() {
    let c = client();
    let reg_id = unique_id("urn:ngsi-ld:ContextSourceRegistration:no-accept-reg");
    let reg = json!({
        "id": &reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "expiresAt": "2035-11-29T14:53:15Z",
        "@context": CORE_CONTEXT
    });

    let status = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = raw_request(&c, Method::GET, "/csourceRegistrations?type=Vehicle", None, None, Some("*/*")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_json_content_type(&headers);
    assert_has_link_header(&headers);

    delete_registration(&c, &reg_id).await;
}

#[tokio::test]
async fn get_without_accept_temporal_entities_returns_json_with_link() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:no-accept-temporal");
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"}
        ],
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
            {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z", "datasetId": "urn:ngsi-ld:Vehicle:12345-fuel"}
        ],
        "@context": CORE_CONTEXT
    });

    let status = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = raw_request(
        &c,
        Method::GET,
        "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
        None,
        None,
        Some("*/*"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_json_content_type(&headers);
    assert_has_link_header(&headers);

    delete_temporal_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Additional tests not in original suite but warranted by NGSI-LD common behaviour
// ---------------------------------------------------------------------------

// NGSI-LD 6.3.4: POST with no Content-Type should be rejected
#[tokio::test]
async fn unsupported_media_type_post_entities_no_content_type_returns_415() {
    let c = client();
    let resp = c
        .post(format!("{BASE_URL}/entities"))
        .body(b"{}".to_vec())
        .send()
        .await
        .expect("request failed");
    // Without Content-Type, broker should reject as unsupported
    let status = resp.status();
    assert!(
        status == StatusCode::UNSUPPORTED_MEDIA_TYPE || status == StatusCode::BAD_REQUEST,
        "POST without Content-Type should return 415 or 400, got {status}"
    );
}

// NGSI-LD 6.3.5: invalid params on GET /attributes
#[tokio::test]
async fn invalid_parameters_get_attributes_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(&c, Method::GET, "/attributes?invalidParam=invalidValue", None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

// NGSI-LD 6.3.5: invalid params on GET /csourceRegistrations
#[tokio::test]
async fn invalid_parameters_get_registrations_returns_400() {
    let c = client();
    let (status, _, body) = raw_request(
        &c,
        Method::GET,
        "/csourceRegistrations?type=Building&invalidParam=invalidValue",
        None,
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

// NGSI-LD 6.3.4: Accept text/plain on entity endpoint -> 406
#[tokio::test]
async fn not_acceptable_get_entity_text_plain_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/entities/urn:ngsi-ld:Building:dummy", None, None, Some("text/plain")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

// NGSI-LD 6.3.4: unsupported Content-Type text/plain on POST
#[tokio::test]
async fn unsupported_media_type_post_entities_text_plain_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::POST, "/entities", Some(b"not json"), Some("text/plain"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

// NGSI-LD 6.3.4: Content-Type application/xml on PATCH entity -> 415
#[tokio::test]
async fn unsupported_media_type_patch_entity_returns_415() {
    let c = client();
    let (status, _, _) = raw_request(
        &c,
        Method::PATCH,
        "/entities/urn:ngsi-ld:Vehicle:dummy",
        Some(XML_DUMMY_BODY),
        Some("application/xml"),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

// NGSI-LD 6.3.4: Accept application/xml on GET /types -> 406
#[tokio::test]
async fn not_acceptable_get_types_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/types", None, None, Some("application/xml")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

// NGSI-LD 6.3.4: Accept application/xml on GET /attributes -> 406
#[tokio::test]
async fn not_acceptable_get_attributes_xml_returns_406() {
    let c = client();
    let (status, _, _) = raw_request(&c, Method::GET, "/attributes", None, None, Some("application/xml")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

// Smoke: creating duplicate entity -> 409 Conflict
#[tokio::test]
async fn smoke_create_duplicate_entity_returns_409() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:smoke-dup");
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "@context": CORE_CONTEXT
    });

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT, "duplicate create should return 409");

    delete_entity(&c, &id).await;
}

// Smoke: GET non-existent entity -> 404
#[tokio::test]
async fn smoke_get_nonexistent_entity_returns_404() {
    let c = client();
    let (status, _) = get_entity(&c, "urn:ngsi-ld:Vehicle:does-not-exist").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

// Smoke: DELETE non-existent entity -> 404
#[tokio::test]
async fn smoke_delete_nonexistent_entity_returns_404() {
    let c = client();
    let status = delete_entity(&c, "urn:ngsi-ld:Vehicle:does-not-exist").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}
