#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Distributed temporal entity retrieve tests.
//!
//! Tests distributed operations on single temporal entity retrieval:
//! local-only retrieval, attribute merging from context sources, error handling.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use andromeda_test_common::{db::TRUNCATE_ALL, prelude::*};
use axum::{extract::Request, response::IntoResponse};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use tokio::sync::RwLock;
use urlencoding::encode;
use uuid::Uuid;

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

struct MockServer {
    port: u16,
    state: Arc<RwLock<MockState>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockServer {
    async fn start() -> Self {
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

    async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        let mut s = self.state.write().await;
        s.stubs.insert(
            (method.to_string(), path.to_string()),
            StubEntry {
                status_code,
                body: None,
                content_type: "application/ld+json".to_string(),
                count: AtomicUsize::new(0),
            },
        );
    }

    async fn set_stub_with_body(&self, method: Method, path: &str, status_code: u16, body: &Value) {
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

    async fn get_count(&self, method: Method, path: &str) -> usize {
        let s = self.state.read().await;
        s.stubs
            .get(&(method.to_string(), path.to_string()))
            .map_or(0, |e| e.count.load(Ordering::Relaxed))
    }

    fn url(&self, path: &str) -> String {
        callback_url(self.port, path)
    }

    fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

fn temporal_vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"}
        ],
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"}
        ],
        "@context": TEST_SUITE_CONTEXT
    })
}

fn vehicle_registration(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {"id": entity_id, "type": "Vehicle"}
                ]
            }
        ],
        "endpoint": endpoint,
        "operations": ["retrieveEntity", "queryEntity", "retrieveTemporalEntity", "queryTemporalEntity"],
        "@context": TEST_SUITE_CONTEXT
    });
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }
    reg
}

async fn create_temporal_entity(client: &Client, entity: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_temporal_entity request failed");
    resp.status()
}

async fn create_registration(client: &Client, reg: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(reg)
        .send()
        .await
        .expect("create_registration request failed");
    resp.status()
}

async fn get_temporal_entity(client: &Client, id: &str, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities/{}{sep}{query}", encode(id)))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("get_temporal_entity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn delete_temporal_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/temporal/entities/{}", encode(id))).send().await;
}

async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

fn warning_header_values(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// local=true retrieves only local temporal entity, skips CSR forwarding.
/// Spec ref: 5.7.3.4 - when local=true, distributed operations are not performed.
#[tokio::test]
#[serial]
async fn distop_temporal_retrieve_local_true_skips_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tr-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tr-local");

    // Create local temporal entity
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    // Create CSR
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Stub retrieve endpoint
    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true
    let (status, _, _) = get_temporal_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Mock should not be called
    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}")).await,
        0,
        "mock should not be called when local=true"
    );

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Retrieve merges local and remote temporal entity attributes.
/// Spec ref: 5.7.3.4 - inclusive mode merges attributes from context sources.
#[tokio::test]
#[serial]
async fn distop_temporal_retrieve_merges_local_and_remote_attributes() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tr-merge");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tr-merge");

    // Create local temporal entity (has speed, fuelLevel)
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    // Create CSR
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns entity with extra attribute (altitude)
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "altitude": [{"type": "Property", "value": 500, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Should have both local (speed, fuelLevel) and remote (altitude) attributes
    assert!(body.get("speed").is_some(), "local attribute 'speed' should be present");
    assert!(body.get("altitude").is_some(), "remote attribute 'altitude' should be merged");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Retrieve returns entity only from remote context source when no local entity exists.
/// Spec ref: 5.7.3.4 - when entity exists only in context source, return from there.
#[tokio::test]
#[serial]
async fn distop_temporal_retrieve_remote_only_entity() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tr-remote-only");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tr-remote-only");

    // No local entity - only exists in remote
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 42, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(body.get("id").and_then(|v| v.as_str()), Some(entity_id.as_str()));
    assert!(body.get("speed").is_some(), "remote entity speed should be present");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Retrieve returns NGSILD-Warning header when context source returns error.
/// Spec ref: 5.7.3.4 - errors from context sources result in 299 warning.
#[tokio::test]
#[serial]
async fn distop_temporal_retrieve_warning_on_context_source_error() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tr-warn");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tr-warn");

    // Create local temporal entity so we get 200 even with CS error
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns 500 error
    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 500).await;

    let (status, headers, _) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header on CS error, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Retrieve with exclusive CSR returns only remote entity (local not considered).
/// Spec ref: 5.7.3.4 - exclusive mode: only context source data is authoritative.
#[tokio::test]
#[serial]
async fn distop_temporal_retrieve_exclusive_mode_returns_remote() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tr-exclusive");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tr-exclusive");

    // Create local temporal entity with different speed values
    let local_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 999, "observedAt": "2020-08-01T12:00:00Z"}],
        "@context": TEST_SUITE_CONTEXT
    });
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &local_entity).await, StatusCode::CREATED);

    // Create CSR in exclusive mode
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), Some("exclusive"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns entity with different speed
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 42, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Should have remote value (42), not local (999)
    let speed = body.get("speed").expect("speed should be present");
    let speed_values: Vec<i64> = speed.as_array().unwrap().iter().filter_map(|v| v["value"].as_i64()).collect();
    assert!(
        speed_values.contains(&42),
        "exclusive mode should return remote speed=42, got: {speed_values:?}"
    );

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}
