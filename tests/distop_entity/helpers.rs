#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Common helpers for distributed entity operation tests.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use andromeda_test_common::config::callback_url;
use axum::{extract::Request, response::IntoResponse};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::RwLock;
use urlencoding::encode;
use uuid::Uuid;

pub use andromeda_test_common::{
    config::{BASE_URL, COMPOUND_CONTEXT as TEST_SUITE_CONTEXT, DATABASE_URL},
    db::TRUNCATE_ALL,
};

// ---------------------------------------------------------------------------
// Mock context source server
// ---------------------------------------------------------------------------

struct StubEntry {
    status_code: u16,
    body: Option<String>,
    content_type: String,
    count: AtomicUsize,
}

struct MockState {
    stubs: HashMap<(String, String), StubEntry>,
    total_count: AtomicUsize,
}

pub struct MockServer {
    port: u16,
    state: Arc<RwLock<MockState>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockServer {
    pub async fn start() -> Self {
        let state = Arc::new(RwLock::new(MockState {
            stubs: HashMap::new(),
            total_count: AtomicUsize::new(0),
        }));

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handler_state = state.clone();
        let app = axum::Router::new().fallback(move |req: Request| {
            let state = handler_state.clone();
            async move {
                let method = req.method().to_string();
                let path = req.uri().path().to_string();

                let s = state.read().await;
                s.total_count.fetch_add(1, Ordering::Relaxed);

                let key = (method, path);
                if let Some(entry) = s.stubs.get(&key) {
                    entry.count.fetch_add(1, Ordering::Relaxed);
                    let status = axum::http::StatusCode::from_u16(entry.status_code).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(ref body) = entry.body {
                        let ct = entry.content_type.clone();
                        (status, [(axum::http::header::CONTENT_TYPE, ct)], body.clone()).into_response()
                    } else {
                        status.into_response()
                    }
                } else {
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        });

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self {
            port,
            state,
            shutdown: Some(shutdown_tx),
        }
    }

    pub async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        let mut s = self.state.write().await;
        s.stubs.insert(
            (method.to_string(), path.to_string()),
            StubEntry {
                status_code,
                body: None,
                content_type: "application/json".to_string(),
                count: AtomicUsize::new(0),
            },
        );
    }

    pub async fn set_stub_with_body(&self, method: Method, path: &str, status_code: u16, body: &Value) {
        let mut s = self.state.write().await;
        s.stubs.insert(
            (method.to_string(), path.to_string()),
            StubEntry {
                status_code,
                body: Some(serde_json::to_string(body).unwrap()),
                content_type: "application/ld+json".to_string(),
                count: AtomicUsize::new(0),
            },
        );
    }

    pub async fn set_stub_with_raw_body(&self, method: Method, path: &str, status_code: u16, body: &str, content_type: &str) {
        let mut s = self.state.write().await;
        s.stubs.insert(
            (method.to_string(), path.to_string()),
            StubEntry {
                status_code,
                body: Some(body.to_string()),
                content_type: content_type.to_string(),
                count: AtomicUsize::new(0),
            },
        );
    }

    pub async fn get_count(&self, method: Method, path: &str) -> usize {
        let s = self.state.read().await;
        s.stubs
            .get(&(method.to_string(), path.to_string()))
            .map_or(0, |e| e.count.load(Ordering::Relaxed))
    }

    pub async fn get_total_count(&self) -> usize {
        let s = self.state.read().await;
        s.total_count.load(Ordering::Relaxed)
    }

    pub fn url(&self, path: &str) -> String {
        callback_url(self.port, path)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

// ---------------------------------------------------------------------------
// Test utilities
// ---------------------------------------------------------------------------

pub fn client() -> Client {
    Client::new()
}

pub fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

pub fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

pub async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Entity builders
// ---------------------------------------------------------------------------

pub fn vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

pub fn vehicle_entity_with_speed(id: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": speed
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

pub fn vehicle_entity_speed_only(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

pub fn vehicle_entity_second(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "BMW"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        }
    })
}

pub fn vehicle_entity_second_ld(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "BMW"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

pub fn vehicle_entity_different_ld(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown3",
            "observedAt": "2018-07-29T12:00:04Z"
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

pub fn vehicle_entity_with_scope(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50
        },
        "scope": scope,
        "@context": TEST_SUITE_CONTEXT
    })
}

/// Vehicle with brandName and isParked (no context, for JSON content type).
pub fn vehicle_entity_json(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z"
        }
    })
}

/// Replacement payload with different attributes (no context, JSON).
pub fn vehicle_replacement_json(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 120
        },
        "fuelType": {
            "type": "Property",
            "value": "diesel"
        }
    })
}

// ---------------------------------------------------------------------------
// Registration builders
// ---------------------------------------------------------------------------

pub fn vehicle_registration(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {"type": "Vehicle"}
                ]
            }
        ],
        "endpoint": endpoint,
        "@context": TEST_SUITE_CONTEXT
    });

    if let Some(eid) = entity_id {
        reg["information"][0]["entities"][0]["id"] = json!(eid);
    }
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

pub fn vehicle_registration_with_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>, operations: &[&str]) -> Value {
    let mut reg = vehicle_registration(reg_id, entity_id, endpoint, mode);
    reg["operations"] = json!(operations);
    reg
}

pub fn vehicle_registration_with_alias(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>, alias: &str) -> Value {
    let mut reg = vehicle_registration(reg_id, entity_id, endpoint, mode);
    reg["contextSourceAlias"] = json!(alias);
    reg
}

pub fn vehicle_registration_speed_property(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>, operations: Option<&[&str]>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "@context": TEST_SUITE_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }
    if let Some(ops) = operations {
        reg["operations"] = json!(ops);
    }

    reg
}

/// Vehicle registration with redirection operations (createEntity, updateEntity, deleteEntity, mergeEntity, replaceEntity).
pub fn vehicle_registration_redirection_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = ["createEntity", "updateEntity", "deleteEntity", "mergeEntity", "replaceEntity"];
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {"type": "Vehicle"}
                ]
            }
        ],
        "endpoint": endpoint,
        "operations": ops,
        "@context": TEST_SUITE_CONTEXT
    });

    if let Some(eid) = entity_id {
        reg["information"][0]["entities"][0]["id"] = json!(eid);
    }
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

/// Speed-specific registration with redirection operations.
pub fn vehicle_registration_speed_redirection_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = ["createEntity", "updateEntity", "deleteEntity", "mergeEntity", "replaceEntity"];
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "operations": ops,
        "@context": TEST_SUITE_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

/// Speed-specific registration with default operations (no redirection ops).
pub fn vehicle_registration_speed_default_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "@context": TEST_SUITE_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

pub async fn create_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

pub async fn create_entity_local(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities?local=true"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity_local request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

pub async fn create_registration(client: &Client, reg: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(reg)
        .send()
        .await
        .expect("create_registration request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

pub async fn get_entity(client: &Client, id: &str, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities/{}{sep}{query}", encode(id)))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities(client: &Client, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities_with_via(client: &Client, query: &str, link: &str, via: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .header("Via", via)
        .send()
        .await
        .expect("query_entities_with_via request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities_post(client: &Client, body: &Value, link: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/query"))
        .header("Accept", "application/ld+json")
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(body)
        .send()
        .await
        .expect("query_entities_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

pub async fn delete_entity_with_status(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}", encode(id)))
        .send()
        .await
        .expect("delete_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn delete_entity_local(client: &Client, id: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}?local=true", encode(id)))
        .send()
        .await
        .expect("delete_entity_local request failed");
    resp.status()
}

pub async fn merge_entity(client: &Client, id: &str, payload: &Value, link: &str) -> (StatusCode, Value) {
    // Strip @context from body - application/json must not contain @context per NGSI-LD 6.3.5
    let mut body = payload.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.remove("@context");
    }
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}", encode(id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(&body)
        .send()
        .await
        .expect("merge_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn replace_entity(client: &Client, id: &str, payload: &Value, link: &str) -> (StatusCode, Value) {
    let resp = client
        .put(format!("{BASE_URL}/entities/{}", encode(id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(payload)
        .send()
        .await
        .expect("replace_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// Warning header extraction
// ---------------------------------------------------------------------------

pub fn warning_header_values(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}
