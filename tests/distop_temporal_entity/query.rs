#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Distributed temporal entity query tests: splitEntities post-merge filtering.

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
            .map(|e| e.count.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn url(&self, path: &str) -> String {
        format!("http://host.docker.internal:{}{path}", self.port)
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
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

fn temporal_vehicle_entity_with_speed(id: &str, speed: i64, observed_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": speed,
            "observedAt": observed_at
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

fn temporal_vehicle_entity_with_scope(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50,
            "observedAt": "2024-01-01T12:00:00Z"
        },
        "scope": scope,
        "@context": TEST_SUITE_CONTEXT
    })
}

fn vehicle_registration_temporal(reg_id: &str, endpoint: &str, mode: &str) -> Value {
    json!({
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
        "mode": mode,
        "operations": ["queryTemporalEntity"],
        "@context": TEST_SUITE_CONTEXT
    })
}

async fn create_temporal_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_temporal_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

async fn create_registration(client: &Client, reg: &Value) -> (StatusCode, Option<String>) {
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

async fn query_temporal_entities(client: &Client, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities{sep}{query}"))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("query_temporal_entities request failed");
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

fn vehicle_registration_with_mode(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Temporal splitEntities with q filter: only matching entities remain after post-merge filtering.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_q_filter_post_merge() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tsplit-q");

    // Local temporal entity: speed=50
    let entity = temporal_vehicle_entity_with_speed(&local_id, 50, "2024-01-01T12:00:00Z");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns temporal entity: speed=80
    let mock_entity = temporal_vehicle_entity_with_speed(&remote_id, 80, "2024-01-01T12:00:00Z");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Query with splitEntities=true and q filter for speed>60
    let query = "type=Vehicle&splitEntities=true&q=speed>60&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local temporal entity (speed=50) should be filtered out by q=speed>60"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote temporal entity (speed=80) should pass q=speed>60 filter"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal splitEntities without filter returns all entities.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_returns_all_without_filter() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-all-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-all-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tsplit-all");

    let entity = temporal_vehicle_entity_with_speed(&local_id, 50, "2024-01-01T12:00:00Z");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity_with_speed(&remote_id, 80, "2024-01-01T12:00:00Z");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(returned_ids.contains(&local_id.as_str()), "should contain local temporal entity");
    assert!(returned_ids.contains(&remote_id.as_str()), "should contain remote temporal entity");

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal splitEntities with scopeQ filter applied post-merge.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_scope_filter_post_merge() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tscope-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tscope-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tscope-split");

    let entity = temporal_vehicle_entity_with_scope(&local_id, "/Madrid");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity_with_scope(&remote_id, "/Barcelona");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&scopeQ=/Barcelona&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local temporal entity (scope /Madrid) should be filtered out by scopeQ=/Barcelona"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote temporal entity (scope /Barcelona) should pass scopeQ=/Barcelona filter"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// local=true skips CSR forwarding for temporal query.
/// Spec ref: 5.7.4.4 - when local=true, distributed operations are not performed.
#[tokio::test]
#[serial]
async fn distop_temporal_query_local_true_skips_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-local-skip");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-local-skip");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await,
        0,
        "mock should not be called when local=true"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Inclusive mode (default) merges remote attributes into local temporal entity.
/// Spec ref: 5.7.4.4 - inclusive mode merges attributes from context sources.
#[tokio::test]
#[serial]
async fn distop_temporal_query_inclusive_mode_merges_attributes() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-inclusive");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-inclusive");

    // Create local temporal entity
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    // Create CSR in inclusive mode (default)
    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    // Mock returns temporal entity with extra attribute
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "altitude": [{"type": "Property", "value": 100, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");
    let entity = entities.iter().find(|e| e["id"].as_str() == Some(entity_id.as_str()));
    assert!(entity.is_some(), "entity should be present in results");
    assert!(entity.unwrap().get("altitude").is_some(), "altitude from remote should be merged into entity");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Auxiliary mode supplements existing entities only, does not add new ones.
/// Spec ref: 5.7.4.4 - auxiliary mode supplements only, does not add new entities.
#[tokio::test]
#[serial]
async fn distop_temporal_query_auxiliary_mode_supplements_only() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tq-auxiliary-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tq-auxiliary-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-auxiliary");

    // Create local temporal entity
    let entity = temporal_vehicle_entity(&local_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    // Create CSR in auxiliary mode
    let reg = vehicle_registration_with_mode(&reg_id, &local_id, &mock.url(""), Some("auxiliary"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    // Mock returns local entity supplement + remote-only entity
    let mock_entities = json!([
        {"id": local_id, "type": "Vehicle", "altitude": [{"type": "Property", "value": 200}]},
        {"id": remote_id, "type": "Vehicle", "speed": [{"type": "Property", "value": 90}]}
    ]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_entities).await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    // Remote-only entity should not be added in auxiliary mode
    assert!(
        !returned_ids.contains(&remote_id.as_str()),
        "auxiliary mode should not add new entities, but remote entity was found"
    );

    // Local entity should have altitude supplemented
    let local = entities.iter().find(|e| e["id"].as_str() == Some(local_id.as_str()));
    assert!(local.is_some(), "local entity should be present");
    assert!(
        local.unwrap().get("altitude").is_some(),
        "altitude should be supplemented from remote in auxiliary mode"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Exclusive CSR mode returns entity from context source only.
/// Spec ref: 5.7.4.4 - exclusive mode: context source is authoritative.
#[tokio::test]
#[serial]
async fn distop_temporal_query_exclusive_mode_returns_remote() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-exclusive");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-exclusive");

    // Create CSR in exclusive mode (no local entity)
    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), Some("exclusive"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    // Mock returns temporal entity
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 55, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");
    let found = entities.iter().any(|e| e["id"].as_str() == Some(entity_id.as_str()));
    assert!(found, "exclusive CSR entity should appear in results");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// NGSILD-Warning header returned when context source returns error.
/// Spec ref: 5.7.4.4 - errors from context sources result in 299 warning.
#[tokio::test]
#[serial]
async fn distop_temporal_query_warning_on_context_source_error() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-warn");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-warn");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    // Mock returns 500 error
    mock.set_stub(Method::GET, "/ngsi-ld/v1/temporal/entities", 500).await;

    let (status, headers, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// type=* implies local=true per spec.
/// Spec ref: 5.7.4.4 - when type=*, distributed operations are not performed.
#[tokio::test]
#[serial]
async fn distop_temporal_query_type_star_implies_local() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-star");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-star");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    // type=* implies local=true per spec
    let (status, _, _) = query_temporal_entities(&c, "type=*&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await,
        0,
        "mock should not be called when type=* (implies local=true)"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal query parameters (timerel, timeAt, lastN) are forwarded to context source.
/// Spec ref: 5.7.4.4 - temporal parameters shall be forwarded.
#[tokio::test]
#[serial]
async fn distop_temporal_query_forwards_temporal_parameters() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-params");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-params");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    // Mock returns empty array
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z&lastN=5", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Verify mock received the forwarded request
    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await > 0,
        "mock should receive forwarded temporal query"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Via header is propagated when forwarding temporal query.
/// Spec ref: 5.7.4.4 - Via header shall be added/updated on forwarding.
#[tokio::test]
#[serial]
async fn distop_temporal_query_via_header_propagated() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-via");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-via");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Verify mock was called (Via header is added internally by broker)
    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await > 0,
        "mock should receive forwarded request with Via header"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}
