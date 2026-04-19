#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Common helpers for distributed entity attribute operation tests.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

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

    pub async fn get_count(&self, method: Method, path: &str) -> usize {
        let s = self.state.read().await;
        s.stubs
            .get(&(method.to_string(), path.to_string()))
            .map_or(0, |e| e.count.load(Ordering::Relaxed))
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://host.docker.internal:{}{path}", self.port)
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
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Entity builders
// ---------------------------------------------------------------------------

/// Vehicle entity with brandName and isParked attributes.
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

/// Vehicle entity with speed and isParked2 attributes (different from standard).
pub fn vehicle_entity_different(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

/// Vehicle entity with two datasetId speed attributes (for delete tests).
pub fn vehicle_entity_two_speed_datasets(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "speed": [
            {
                "type": "Property",
                "value": 50,
                "datasetId": "urn:ngsi-ld:Dataset:speed:1"
            },
            {
                "type": "Property",
                "value": 60
            }
        ],
        "@context": TEST_SUITE_CONTEXT
    })
}

// ---------------------------------------------------------------------------
// Fragment builders (attribute-level payloads)
// ---------------------------------------------------------------------------

/// Fragment with speed and isParked for append operations.
pub fn fragment_speed_isparked() -> Value {
    json!({
        "speed": {
            "type": "Property",
            "value": 120
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2018-01-01T12:00:00Z"
        }
    })
}

/// Fragment with speed for update/partial update operations.
pub fn fragment_speed_different() -> Value {
    json!({
        "speed": {
            "type": "Property",
            "value": 200
        }
    })
}

/// Fragment with speed with datasetId for partial update operations.
pub fn fragment_speed_datasetid() -> Value {
    json!({
        "type": "Property",
        "value": 150,
        "datasetId": "urn:ngsi-ld:Dataset:speed:updated"
    })
}

/// Fragment with brandName for update operations.
pub fn fragment_brandname() -> Value {
    json!({
        "brandName": {
            "type": "Property",
            "value": "BMW"
        }
    })
}

// ---------------------------------------------------------------------------
// Registration builders
// ---------------------------------------------------------------------------

/// Vehicle registration with entity and attribute redirection operations.
///
/// Includes both entity-level operations (createEntity, updateEntity, etc.) and
/// attribute-level operations (appendAttrs, updateAttrs, partialAttrUpdate, deleteAttrs).
pub fn vehicle_registration_redirection_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = [
        "createEntity",
        "updateEntity",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "appendAttrs",
        "updateAttrs",
        "partialAttrUpdate",
        "deleteAttrs",
    ];
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

/// Speed-specific registration with entity and attribute redirection operations.
///
/// Includes both entity-level and attribute-level operations for the speed property.
pub fn vehicle_registration_speed_redirection_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = [
        "createEntity",
        "updateEntity",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "appendAttrs",
        "updateAttrs",
        "partialAttrUpdate",
        "deleteAttrs",
    ];
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

// ---------------------------------------------------------------------------
// HTTP helpers - Entity operations
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

pub async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// HTTP helpers - Entity attribute operations
// ---------------------------------------------------------------------------

/// Append entity attributes (POST /entities/{entityId}/attrs).
pub async fn append_entity_attrs(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities/{}/attrs", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("append_entity_attrs request failed");
    resp.status()
}

/// Append entity attributes with noOverwrite option.
pub async fn append_entity_attrs_no_overwrite(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities/{}/attrs?options=noOverwrite", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("append_entity_attrs_no_overwrite request failed");
    resp.status()
}

/// Update entity attributes (PATCH /entities/{entityId}/attrs).
pub async fn update_entity_attrs(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}/attrs", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("update_entity_attrs request failed");
    resp.status()
}

/// Partial update of entity attribute (PATCH /entities/{entityId}/attrs/{attrId}).
pub async fn partial_update_attr(client: &Client, entity_id: &str, attr_name: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}/attrs/{}", encode(entity_id), encode(attr_name)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("partial_update_attr request failed");
    resp.status()
}

/// Delete entity attribute (DELETE /entities/{entityId}/attrs/{attrId}).
pub async fn delete_entity_attr(client: &Client, entity_id: &str, attr_name: &str, link: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}/attrs/{}", encode(entity_id), encode(attr_name)))
        .header("Link", link)
        .send()
        .await
        .expect("delete_entity_attr request failed");
    resp.status()
}

/// Delete entity attribute with deleteAll=true option.
pub async fn delete_entity_attr_all(client: &Client, entity_id: &str, attr_name: &str, link: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}/attrs/{}?deleteAll=true", encode(entity_id), encode(attr_name)))
        .header("Link", link)
        .send()
        .await
        .expect("delete_entity_attr_all request failed");
    resp.status()
}

// ---------------------------------------------------------------------------
// HTTP helpers - Registration operations
// ---------------------------------------------------------------------------

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

pub async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}
