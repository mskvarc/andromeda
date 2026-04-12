//! Distributed entity query tests: entityMap with forwarded IDs and splitEntities filtering.

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
use serial_test::serial;
use sqlx::PgPool;
use tokio::sync::RwLock;
use urlencoding::encode;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

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

fn vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

fn vehicle_entity_with_speed(id: &str, speed: i64) -> Value {
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

fn vehicle_entity_with_scope(id: &str, scope: &str) -> Value {
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

fn vehicle_registration(reg_id: &str, endpoint: &str, mode: &str) -> Value {
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
        "operations": ["queryEntity"],
        "@context": TEST_SUITE_CONTEXT
    })
}

async fn create_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
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

async fn query_entities(client: &Client, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// EntityMap from distributed query includes both local and forwarded entity IDs.
#[tokio::test]
#[serial]
async fn distop_entity_map_includes_forwarded_entity_ids() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:distop-map");

    // Create local entity
    let entity = vehicle_entity(&local_id);
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Mock returns remote entity
    let mock_entity = vehicle_entity(&remote_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    // Register context source
    let reg = vehicle_registration(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query with entityMap=true
    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true", &jsonld_link_value()).await;
    assert_eq!(status, StatusCode::CREATED, "entityMap=true should return 201 CREATED");

    // Check NGSILD-EntityMap header is present
    let entity_map_header = headers.get("ngsild-entitymap");
    assert!(entity_map_header.is_some(), "NGSILD-EntityMap header should be present");

    // Retrieve the entity map to check its contents
    let map_id = entity_map_header.unwrap().to_str().unwrap();
    let resp = c
        .get(format!("{BASE_URL}/entityMaps/{}", encode(map_id)))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get entity map failed");
    let map_body: Value = resp.json().await.unwrap_or_default();

    // EntityMap should contain both local and remote IDs
    let ids = map_body["ids"].as_array().expect("ids should be an array");
    let id_strings: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();
    assert!(id_strings.contains(&local_id.as_str()), "EntityMap should contain local entity ID");
    assert!(
        id_strings.contains(&remote_id.as_str()),
        "EntityMap should contain remote (forwarded) entity ID"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// EntityMap with local=true only contains local entity IDs.
#[tokio::test]
#[serial]
async fn distop_entity_map_local_only_when_local_true() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:local-only");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:remote-only");
    let reg_id = unique_id("urn:ngsi-ld:Registration:local-map");

    let entity = vehicle_entity(&local_id);
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&remote_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query with local=true -- should not forward
    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true&local=true", &jsonld_link_value()).await;
    assert_eq!(status, StatusCode::CREATED);

    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap();
    let resp = c
        .get(format!("{BASE_URL}/entityMaps/{}", encode(map_id)))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get entity map failed");
    let map_body: Value = resp.json().await.unwrap_or_default();

    let ids = map_body["ids"].as_array().expect("ids should be an array");
    let id_strings: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();
    assert!(id_strings.contains(&local_id.as_str()), "should contain local ID");
    assert!(!id_strings.contains(&remote_id.as_str()), "should NOT contain remote ID when local=true");

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// splitEntities with q filter applies post-merge: only matching entities remain.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_q_filter_post_merge() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:split-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:split-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:split-q");

    // Local: speed=50, remote: speed=80
    let entity = vehicle_entity_with_speed(&local_id, 50);
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_speed(&remote_id, 80);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query with splitEntities=true and q filter for speed>60
    let query = "type=Vehicle&splitEntities=true&q=speed>60";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local entity (speed=50) should be filtered out by q=speed>60"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote entity (speed=80) should pass q=speed>60 filter"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// splitEntities without filter returns all entities from both sources.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_returns_all_without_filter() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:split-all-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:split-all-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:split-all");

    let entity = vehicle_entity_with_speed(&local_id, 50);
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_speed(&remote_id, 80);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(returned_ids.contains(&local_id.as_str()), "should contain local entity");
    assert!(returned_ids.contains(&remote_id.as_str()), "should contain remote entity");

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// splitEntities with scopeQ filter applied post-merge.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_scope_filter_post_merge() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:scope-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:scope-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:scope-split");

    // Local: scope /Madrid, remote: scope /Barcelona
    let entity = vehicle_entity_with_scope(&local_id, "/Madrid");
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_scope(&remote_id, "/Barcelona");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&scopeQ=/Barcelona";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local entity (scope /Madrid) should be filtered out by scopeQ=/Barcelona"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote entity (scope /Barcelona) should pass scopeQ=/Barcelona filter"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}
