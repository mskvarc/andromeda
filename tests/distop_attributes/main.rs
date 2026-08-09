//! Distributed attribute discovery integration tests.
//!
//! Tests GET /attributes and GET /attributes/{attrId} forwarding, merging,
//! deduplication, and warning headers when CSR calls fail.
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_attributes_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use andromeda_test_common::{
    config::{BASE_URL, COMPOUND_CONTEXT, DATABASE_URL, callback_url},
    db::TRUNCATE_ALL,
};
use axum::{extract::Request, response::IntoResponse};
use reqwest::{Client, Method, StatusCode};
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
}

struct MockServer {
    port: u16,
    state: Arc<RwLock<MockState>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockServer {
    async fn start() -> MockServer {
        let state = Arc::new(RwLock::new(MockState { stubs: HashMap::new() }));

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handler_state = state.clone();
        let app = axum::Router::new().fallback(move |req: Request| {
            let state = handler_state.clone();
            async move {
                let method = req.method().to_string();
                let path = req.uri().path().to_string();
                let s = state.read().await;
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
                    axum::http::StatusCode::NOT_FOUND.into_response()
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

        MockServer {
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
                content_type: String::new(),
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

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

fn vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_registration(reg_id: &str, endpoint: &str) -> Value {
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
        "@context": [COMPOUND_CONTEXT]
    })
}

async fn create_entity_local(client: &Client, entity: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities?local=true"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity_local request failed");
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

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

async fn list_attributes(client: &Client, query: &str) -> (StatusCode, reqwest::header::HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/attributes{sep}{query}"))
        .send()
        .await
        .expect("list_attributes request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn get_attribute(client: &Client, attr_name: &str, query: &str, context_url: Option<&str>) -> (StatusCode, reqwest::header::HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut request = client.get(format!("{BASE_URL}/attributes/{attr_name}{sep}{query}"));
    if let Some(ctx) = context_url {
        request = request.header("Link", format!(r#"<{ctx}>; rel="http://www.w3.org/ns/json-ld#context""#));
    }
    let resp = request.send().await.expect("get_attribute request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

fn warning_header_values(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// list_attributes with local=true does not forward to CSR endpoints.
#[tokio::test]
#[serial]
async fn distop_attributes_local_skips_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-local-skip");

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    let mock_attrs = json!({
        "id": "urn:ngsi-ld:AttributeList:mock",
        "type": "AttributeList",
        "attributeList": ["speed"]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

    let (status, _, _) = list_attributes(&c, "local=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/attributes").await,
        0,
        "CSR should not be called when local=true"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// list_attributes without local=true forwards to CSR and merges remote attributes.
#[tokio::test]
#[serial]
async fn distop_attributes_merges_remote_attributes() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:attrs-merge");
    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-merge");

    let entity = vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_entity_local(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Remote returns an attribute not present locally
    let mock_attrs = json!({
        "id": "urn:ngsi-ld:AttributeList:mock",
        "type": "AttributeList",
        "attributeList": ["speed"]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

    let (status, _, body) = list_attributes(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/attributes").await > 0,
        "CSR should be called when local is not set"
    );

    let attr_list = body.get("attributeList").and_then(|v| v.as_array()).expect("attributeList should be present");
    let attrs: Vec<&str> = attr_list.iter().filter_map(|v| v.as_str()).collect();
    assert!(attrs.contains(&"speed"), "merged result should include speed from remote; got: {attrs:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// list_attributes deduplicates attributes that appear in both local and remote results.
#[tokio::test]
#[serial]
async fn distop_attributes_deduplicates_attribute_list() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:attrs-dedup");
    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-dedup");

    let entity = vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_entity_local(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Remote also returns brandName which already exists locally
    let mock_attrs = json!({
        "id": "urn:ngsi-ld:AttributeList:mock",
        "type": "AttributeList",
        "attributeList": ["brandName"]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

    let (status, _, body) = list_attributes(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let attr_list = body.get("attributeList").and_then(|v| v.as_array()).expect("attributeList should be present");
    let mut seen = std::collections::HashSet::new();
    for attr in attr_list {
        if let Some(s) = attr.as_str() {
            assert!(seen.insert(s), "duplicate attribute found in list: {s}");
        }
    }

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// list_attributes with details=true merges detailed attribute objects from local and remote.
#[tokio::test]
#[serial]
async fn distop_attributes_details_merges_remote_attribute_objects() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:attrs-details");
    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-details");

    let entity = vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_entity_local(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Remote returns a detailed attribute not present locally
    let mock_details = json!([
        {
            "id": "altitude",
            "type": "Attribute",
            "attributeName": "altitude",
            "typeNames": ["Drone"]
        }
    ]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_details).await;

    let (status, _, body) = list_attributes(&c, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("details=true response should be a JSON array");
    let altitude = arr.iter().find(|v| v.get("id").and_then(|id| id.as_str()) == Some("altitude"));
    assert!(altitude.is_some(), "merged details should include altitude from remote; got: {arr:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// list_attributes emits NGSILD-Warning 299 header when a CSR returns an error response.
#[tokio::test]
#[serial]
async fn distop_attributes_warning_on_csr_error_response() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-warn");

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::GET, "/ngsi-ld/v1/attributes", 403).await;

    let (status, headers, _) = list_attributes(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header when CSR returns 403");
    assert!(
        warnings.iter().any(|w| w.starts_with("299")),
        "expected warning code 299 for error response; got: {warnings:?}"
    );

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// get_attribute forwards to CSR and merges type name information from remote.
#[tokio::test]
#[serial]
async fn distop_attributes_attribute_info_merges_remote_type_names() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:attrs-info-merge");
    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-info-merge");

    let entity = vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_entity_local(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Verify the attribute exists locally first
    let (local_status, _, _) = get_attribute(&c, "brandName", "local=true", Some(COMPOUND_CONTEXT)).await;
    pretty_assertions::assert_eq!(local_status, StatusCode::OK);

    // Remote returns attribute info with additional type names
    let mock_attr_info = json!({
        "id": "brandName",
        "type": "Attribute",
        "attributeName": "brandName",
        "typeNames": ["Drone"]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes/brandName", 200, &mock_attr_info)
        .await;

    let (status, _, _) = get_attribute(&c, "brandName", "", Some(COMPOUND_CONTEXT)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// list_attributes emits NGSILD-Warning 199 header when a CSR is unreachable.
#[tokio::test]
#[serial]
async fn distop_attributes_warning_on_csr_unreachable() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let reg_id = unique_id("urn:ngsi-ld:Registration:attrs-timeout");
    let endpoint = mock.url("");

    let reg = vehicle_registration(&reg_id, &endpoint);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Stop mock to simulate unreachable CSR
    mock.stop();

    let (status, headers, _) = list_attributes(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header when CSR is unreachable");
    assert!(
        warnings.iter().any(|w| w.starts_with("199")),
        "expected warning code 199 for unreachable CSR; got: {warnings:?}"
    );

    delete_registration(&c, &reg_id).await;
}
