//! Distributed temporal entity map integration tests.
//!
//! Tests entityMap with forwarded IDs for temporal queries.
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_temporal_entity_map_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use andromeda_test_common::{
    config::{BASE_URL, DATABASE_URL, TEST_SUITE_CONTEXT, callback_url},
    db::TRUNCATE_ALL,
};
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
        "speed": {
            "type": "Property",
            "value": 50,
            "observedAt": "2024-01-01T12:00:00Z"
        },
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
        "operations": ["queryTemporal"],
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

async fn delete_entity_map(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entityMaps/{}", encode(id))).send().await;
}

async fn get_temporal_entity_with_entity_map(client: &Client, id: &str, entity_map_id: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities/{}", encode(id)))
        .header("Accept", "application/json")
        .header("Link", link)
        .header("NGSILD-EntityMap", entity_map_id)
        .send()
        .await
        .expect("get_temporal_entity_with_entity_map request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn get_entity_map(client: &Client, id: &str, link: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("get_entity_map request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Temporal EntityMap includes both local and forwarded entity IDs.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_includes_forwarded_ids() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tmap-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tmap-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tmap");

    // Create local temporal entity
    let entity = temporal_vehicle_entity(&local_id);
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns remote temporal entity
    let mock_entity = temporal_vehicle_entity(&remote_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    // Register context source
    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Query with entityMap=true
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "entityMap=true should return 201 CREATED");

    let entity_map_header = headers.get("ngsild-entitymap");
    assert!(entity_map_header.is_some(), "NGSILD-EntityMap header should be present");

    // Retrieve the entity map
    let map_id = entity_map_header.unwrap().to_str().unwrap();
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
    assert!(id_strings.contains(&local_id.as_str()), "EntityMap should contain local temporal entity ID");
    assert!(
        id_strings.contains(&remote_id.as_str()),
        "EntityMap should contain remote (forwarded) temporal entity ID"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal EntityMap with local=true only contains local IDs.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_local_only_when_local_true() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tmap-local-only");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tmap-remote-only");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tmap-local");

    let entity = temporal_vehicle_entity(&local_id);
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity(&remote_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Query with local=true
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z&local=true",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

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

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal EntityMap scopes subsequent query to map members.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_scopes_subsequent_query() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tmap-scope-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tmap-scope-remote");
    let extra_id = unique_id("urn:ngsi-ld:Vehicle:tmap-scope-extra");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tmap-scope");

    // Create two local temporal entities
    let entity = temporal_vehicle_entity(&local_id);
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let extra_entity = temporal_vehicle_entity(&extra_id);
    let (status, _) = create_temporal_entity(&c, &extra_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns remote temporal entity
    let mock_entity = temporal_vehicle_entity(&remote_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "auxiliary");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create entity map (includes local_id, remote_id, extra_id)
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap();

    // Query with NGSILD-EntityMap header to scope results
    let resp = c
        .get(format!("{BASE_URL}/temporal/entities?type=Vehicle&local=true"))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .header("NGSILD-EntityMap", map_id)
        .send()
        .await
        .expect("scoped query failed");
    let scoped_status = resp.status();
    let scoped_body: Value = resp.json().await.unwrap_or_default();

    assert!(scoped_status.is_success(), "scoped query should succeed");
    let entities = scoped_body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    // All returned IDs should be members of the entity map
    for id in &returned_ids {
        assert!(*id == local_id || *id == extra_id, "returned ID {id} should be a member of the entity map");
    }

    delete_temporal_entity(&c, &local_id).await;
    delete_temporal_entity(&c, &extra_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Temporal query with entityMap=true creates EntityMap and returns 201.
/// Spec ref: 5.7.4.4 - entityMap=true creates new EntityMap, returns 201 Created.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_creates_map_returns_201() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tmap-create");

    let entity = temporal_vehicle_entity(&entity_id);
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "entityMap=true should yield 201 Created");

    let map_header = headers.get("ngsild-entitymap").expect("NGSILD-EntityMap header must be present");
    let map_id = map_header.to_str().unwrap();
    assert!(map_id.starts_with("urn:ngsi-ld:EntityMap:"), "EntityMap ID should be a URN");

    delete_temporal_entity(&c, &entity_id).await;
    delete_entity_map(&c, map_id).await;
}

/// NGSILD-EntityMap header is echoed in query response when using entity map.
/// Spec ref: 5.7.4 - the EntityMap used in operation shall be returned in response header.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_echoed_in_response() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tmap-echo");

    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    // Create EntityMap
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap().to_string();

    // Query again with EntityMap header
    let resp = c
        .get(format!("{BASE_URL}/temporal/entities?type=Vehicle"))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .header("NGSILD-EntityMap", &map_id)
        .send()
        .await
        .expect("query with entity map failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::OK);

    let echoed = resp
        .headers()
        .get("ngsild-entitymap")
        .expect("NGSILD-EntityMap should be echoed in response")
        .to_str()
        .unwrap();
    pretty_assertions::assert_eq!(echoed, map_id, "echoed EntityMap ID should match the one sent");

    delete_temporal_entity(&c, &entity_id).await;
    delete_entity_map(&c, &map_id).await;
}

/// Retrieve returns 404 when entity not in EntityMap.
/// Spec ref: 5.7.3.4 - entity not in map returns 404.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_retrieve_not_in_map_returns_404() {
    clean_database().await;
    let c = client();

    let entity_a = unique_id("urn:ngsi-ld:Vehicle:tmap-scope-a");
    let entity_b = unique_id("urn:ngsi-ld:Vehicle:tmap-scope-b");

    // Create entity A only
    let fixture_a = temporal_vehicle_entity(&entity_a);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_a).await.0, StatusCode::CREATED);

    // Create EntityMap containing only entity A
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap().to_string();

    // Create entity B (not in map)
    let fixture_b = temporal_vehicle_entity(&entity_b);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_b).await.0, StatusCode::CREATED);

    // Retrieve entity B using EntityMap that only contains A
    let (status, _, _) = get_temporal_entity_with_entity_map(&c, &entity_b, &map_id, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "entity B is not in the map, should be 404");

    delete_temporal_entity(&c, &entity_a).await;
    delete_temporal_entity(&c, &entity_b).await;
    delete_entity_map(&c, &map_id).await;
}

/// Retrieve succeeds and echoes EntityMap header when entity is in map.
/// Spec ref: 5.7.3.4 - entity in map returns 200 with NGSILD-EntityMap echoed.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_retrieve_in_map_succeeds() {
    clean_database().await;
    let c = client();

    let entity_a = unique_id("urn:ngsi-ld:Vehicle:tmap-retrieve-pass");

    let fixture_a = temporal_vehicle_entity(&entity_a);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_a).await.0, StatusCode::CREATED);

    // Create EntityMap containing entity A
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap().to_string();

    // Retrieve entity A using EntityMap
    let (status, headers, body) = get_temporal_entity_with_entity_map(&c, &entity_a, &map_id, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "entity A is in the map, should return 200");

    pretty_assertions::assert_eq!(body["id"].as_str(), Some(entity_a.as_str()));

    let echoed = headers
        .get("ngsild-entitymap")
        .expect("NGSILD-EntityMap should be echoed in response")
        .to_str()
        .unwrap();
    pretty_assertions::assert_eq!(echoed, map_id, "echoed EntityMap ID should match");

    delete_temporal_entity(&c, &entity_a).await;
    delete_entity_map(&c, &map_id).await;
}

/// entityMapLifetime parameter sets EntityMap expiry.
/// Spec ref: 5.5.14 - entityMapLifetime suggests an expiry duration.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_with_lifetime_sets_expiry() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tmap-lifetime");

    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    // Create EntityMap with 5 minute lifetime
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z&entityMapLifetime=PT5M",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap().to_string();

    // Retrieve EntityMap and verify expiresAt
    let (status, body) = get_entity_map(&c, &map_id, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expires_at_str = body["expiresAt"].as_str().expect("expiresAt should be present");
    let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at_str).expect("expiresAt should be valid RFC3339");
    let now = chrono::Utc::now();
    let diff = expires_at.signed_duration_since(now);
    assert!(
        diff.num_seconds() > 180 && diff.num_seconds() <= 360,
        "expiresAt should be approximately 5 minutes from now, got {} seconds",
        diff.num_seconds()
    );

    delete_temporal_entity(&c, &entity_id).await;
    delete_entity_map(&c, &map_id).await;
}

/// Query scoped by EntityMap only returns entities in the map.
/// Spec ref: 5.5.14 - requests with NGSILD-EntityMap header filtered to map members.
#[tokio::test]
#[serial]
async fn distop_temporal_entity_map_query_scoped_to_members() {
    clean_database().await;
    let c = client();

    let entity_a = unique_id("urn:ngsi-ld:Vehicle:tmap-query-scope-a");
    let entity_b = unique_id("urn:ngsi-ld:Vehicle:tmap-query-scope-b");

    let fixture_a = temporal_vehicle_entity(&entity_a);
    let fixture_b = temporal_vehicle_entity(&entity_b);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_a).await.0, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_b).await.0, StatusCode::CREATED);

    // Create EntityMap containing A and B
    let (status, headers, _) = query_temporal_entities(
        &c,
        "type=Vehicle&entityMap=true&timerel=before&timeAt=2030-01-01T00:00:00Z",
        &jsonld_link_value(),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap().to_string();

    // Create entity C after map was frozen
    let entity_c = unique_id("urn:ngsi-ld:Vehicle:tmap-query-scope-c");
    let fixture_c = temporal_vehicle_entity(&entity_c);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &fixture_c).await.0, StatusCode::CREATED);

    // Query with frozen EntityMap - only A and B should appear
    let resp = c
        .get(format!("{BASE_URL}/temporal/entities?type=Vehicle"))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .header("NGSILD-EntityMap", &map_id)
        .send()
        .await
        .expect("scoped query failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap_or_default();
    let entities = body.as_array().expect("response should be array");
    let ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(ids.contains(&entity_a.as_str()), "entity A should be in scoped results");
    assert!(ids.contains(&entity_b.as_str()), "entity B should be in scoped results");
    assert!(!ids.contains(&entity_c.as_str()), "entity C should NOT be in scoped results (not in map)");

    delete_temporal_entity(&c, &entity_a).await;
    delete_temporal_entity(&c, &entity_b).await;
    delete_temporal_entity(&c, &entity_c).await;
    delete_entity_map(&c, &map_id).await;
}
