//! Distributed batch entity operation integration tests.
//!
//! Tests batch create, upsert (replace and update), and delete forwarding
//! to CSR endpoints for exclusive, inclusive, and redirect registration modes.
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_batch_<operation>_<mode>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{extract::Request, response::IntoResponse};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use tokio::sync::RwLock;
use urlencoding::encode;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const COMPOUND_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

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
                    axum::http::StatusCode::from_u16(entry.status_code)
                        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .into_response()
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

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

fn link_header() -> String {
    format!("<{COMPOUND_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Vehicle entity with brandName=Mercedes and isParked relationship.
fn vehicle_entity_mercedes(id: &str) -> Value {
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

/// Vehicle entity with brandName=BMW and isParked2 relationship.
fn vehicle_entity_bmw(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "BMW"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle entity with speed property and isParked2 relationship (different attributes from mercedes).
fn vehicle_entity_speed_different(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56,
            "datasetId": "urn:ngsi-ld:Property:speedometerA4567-speed2"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

/// CSR with batch operations covering all Vehicle entities (inclusive by default).
fn batch_ops_registration(reg_id: &str, endpoint: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "idPattern": "urn:ngsi-ld:Vehicle:*",
                        "type": "Vehicle"
                    }
                ]
            }
        ],
        "operations": ["redirectionOps", "createBatch", "upsertBatch", "updateBatch", "deleteBatch", "mergeBatch"],
        "endpoint": endpoint,
        "@context": [COMPOUND_CONTEXT]
    })
}

/// CSR with batch operations, redirect mode, covering all Vehicle entities.
fn batch_ops_redirect_registration(reg_id: &str, endpoint: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "idPattern": "urn:ngsi-ld:Vehicle:*",
                        "type": "Vehicle"
                    }
                ]
            }
        ],
        "operations": ["redirectionOps", "createBatch", "upsertBatch", "updateBatch", "deleteBatch", "mergeBatch"],
        "mode": "redirect",
        "endpoint": endpoint,
        "@context": [COMPOUND_CONTEXT]
    })
}

/// CSR with exclusive mode, covering only speed property for Vehicle entities.
fn batch_ops_exclusive_speed_registration(reg_id: &str, endpoint: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "idPattern": "urn:ngsi-ld:Vehicle:*",
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "operations": ["redirectionOps", "createBatch", "upsertBatch", "updateBatch", "deleteBatch", "mergeBatch"],
        "mode": "exclusive",
        "endpoint": endpoint,
        "@context": [COMPOUND_CONTEXT]
    })
}

async fn create_entity(client: &Client, entity: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity request failed");
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

async fn batch_create(client: &Client, entities: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(entities)
        .send()
        .await
        .expect("batch_create request failed");
    resp.status()
}

async fn batch_upsert_replace(client: &Client, entities: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/upsert?options=replace"))
        .header("Content-Type", "application/ld+json")
        .json(entities)
        .send()
        .await
        .expect("batch_upsert_replace request failed");
    resp.status()
}

async fn batch_upsert_update(client: &Client, entities: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/upsert?options=update"))
        .header("Content-Type", "application/ld+json")
        .json(entities)
        .send()
        .await
        .expect("batch_upsert_update request failed");
    resp.status()
}

async fn batch_delete(client: &Client, ids: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/delete"))
        .header("Content-Type", "application/ld+json")
        .json(ids)
        .send()
        .await
        .expect("batch_delete request failed");
    resp.status()
}

async fn query_vehicle_entities(client: &Client) -> Vec<Value> {
    let resp = client
        .get(format!("{BASE_URL}/entities?type=Vehicle"))
        .header("Accept", "application/json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query_vehicle_entities request failed");
    resp.json::<Vec<Value>>().await.unwrap_or_default()
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// Batch Create Tests
// ---------------------------------------------------------------------------

/// Batch create with an exclusive CSR forwards the operation to the CSR endpoint.
#[tokio::test]
#[serial]
async fn distop_batch_create_exclusive_forwards_to_csr() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-create-exc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-create-exc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-create-exc");

    // Create 2 entities locally first (without speed, so they exist for the exclusive CSR to intercept)
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_exclusive_speed_registration(&reg_id, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create", 201).await;

    // Batch create entities with speed attribute (matches exclusive CSR scope)
    let batch = json!([vehicle_entity_speed_different(&entity_id1), vehicle_entity_speed_different(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_create(&c, &batch).await, StatusCode::CREATED);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create").await,
        1,
        "exclusive CSR should receive exactly one forwarded batch create"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch create with an inclusive CSR stores entities locally and forwards to the CSR.
#[tokio::test]
#[serial]
async fn distop_batch_create_inclusive_stores_locally_and_forwards() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-create-inc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-create-inc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-create-inc");

    let reg = batch_ops_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/create", 201).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_create(&c, &batch).await, StatusCode::CREATED);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/create").await,
        1,
        "inclusive CSR should receive exactly one forwarded batch create"
    );

    let entities = query_vehicle_entities(&c).await;
    assert!(
        entities.iter().any(|e| e["id"] == entity_id1),
        "entity1 should exist locally after batch create"
    );
    assert!(
        entities.iter().any(|e| e["id"] == entity_id2),
        "entity2 should exist locally after batch create"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch create with two redirect CSRs forwards the operation to both CSR endpoints.
#[tokio::test]
#[serial]
async fn distop_batch_create_redirect_forwards_to_both_csrs() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-create-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-create-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:batch-create-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:batch-create-red-2");

    let reg1 = batch_ops_redirect_registration(&reg_id1, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg1).await, StatusCode::CREATED);

    let reg2 = batch_ops_redirect_registration(&reg_id2, &mock.url("/broker2"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg2).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create", 201).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/create", 201).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_create(&c, &batch).await, StatusCode::CREATED);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create").await,
        1,
        "broker1 should receive exactly one forwarded batch create"
    );
    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/create").await,
        1,
        "broker2 should receive exactly one forwarded batch create"
    );

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Batch Upsert (replace) Tests
// ---------------------------------------------------------------------------

/// Batch upsert replace with an exclusive CSR forwards the operation to the CSR endpoint.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_replace_exclusive_forwards_to_csr() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-exc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-exc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-upsert-rep-exc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_exclusive_speed_registration(&reg_id, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert", 204).await;

    // Upsert with speed entities (matches exclusive CSR propertyNames scope)
    let batch = json!([vehicle_entity_speed_different(&entity_id1), vehicle_entity_speed_different(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_replace(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "exclusive CSR should receive exactly one forwarded batch upsert"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch upsert replace with an inclusive CSR updates entities locally and forwards to the CSR.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_replace_inclusive_updates_locally_and_forwards() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-inc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-inc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-upsert-rep-inc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_replace(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "inclusive CSR should receive exactly one forwarded batch upsert"
    );

    let entities = query_vehicle_entities(&c).await;
    let entity = entities.iter().find(|e| e["id"] == entity_id1).expect("entity1 not found after upsert");
    pretty_assertions::assert_eq!(entity["brandName"]["value"], "BMW", "entity brandName should be BMW after replace upsert");

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch upsert replace with two redirect CSRs forwards the operation to both CSR endpoints.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_replace_redirect_forwards_to_both_csrs() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-rep-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:batch-upsert-rep-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:batch-upsert-rep-red-2");

    let reg1 = batch_ops_redirect_registration(&reg_id1, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg1).await, StatusCode::CREATED);

    let reg2 = batch_ops_redirect_registration(&reg_id2, &mock.url("/broker2"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg2).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert", 204).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_replace(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "broker1 should receive exactly one forwarded batch upsert"
    );
    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "broker2 should receive exactly one forwarded batch upsert"
    );

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Batch Upsert (update) Tests
// ---------------------------------------------------------------------------

/// Batch upsert update with an exclusive CSR forwards the operation to the CSR endpoint.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_update_exclusive_forwards_to_csr() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-exc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-exc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-upsert-upd-exc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_exclusive_speed_registration(&reg_id, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let batch = json!([vehicle_entity_speed_different(&entity_id1), vehicle_entity_speed_different(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_update(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "exclusive CSR should receive exactly one forwarded batch upsert (update)"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch upsert update with an inclusive CSR merges attribute changes locally and forwards to the CSR.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_update_inclusive_merges_locally_and_forwards() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-inc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-inc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-upsert-upd-inc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_mercedes(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_update(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "inclusive CSR should receive exactly one forwarded batch upsert (update)"
    );

    let entities = query_vehicle_entities(&c).await;
    let entity = entities.iter().find(|e| e["id"] == entity_id1).expect("entity1 not found after upsert update");
    pretty_assertions::assert_eq!(entity["brandName"]["value"], "BMW", "entity brandName should be BMW after update upsert");

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch upsert update with two redirect CSRs forwards the operation to both CSR endpoints.
#[tokio::test]
#[serial]
async fn distop_batch_upsert_update_redirect_forwards_to_both_csrs() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-upsert-upd-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:batch-upsert-upd-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:batch-upsert-upd-red-2");

    let reg1 = batch_ops_redirect_registration(&reg_id1, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg1).await, StatusCode::CREATED);

    let reg2 = batch_ops_redirect_registration(&reg_id2, &mock.url("/broker2"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg2).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert", 204).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let batch = json!([vehicle_entity_bmw(&entity_id1), vehicle_entity_bmw(&entity_id2)]);
    pretty_assertions::assert_eq!(batch_upsert_update(&c, &batch).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "broker1 should receive exactly one forwarded batch upsert (update)"
    );
    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/upsert").await,
        1,
        "broker2 should receive exactly one forwarded batch upsert (update)"
    );

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Batch Delete Tests
// ---------------------------------------------------------------------------

/// Batch delete with an inclusive CSR deletes entities locally and forwards to the CSR.
#[tokio::test]
#[serial]
async fn distop_batch_delete_inclusive_deletes_locally_and_forwards() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-inc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-inc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-delete-inc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_registration(&reg_id, &mock.url(""));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/delete", 204).await;

    let ids = json!([entity_id1, entity_id2]);
    pretty_assertions::assert_eq!(batch_delete(&c, &ids).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/delete").await,
        1,
        "inclusive CSR should receive exactly one forwarded batch delete"
    );

    let entities = query_vehicle_entities(&c).await;
    assert!(!entities.iter().any(|e| e["id"] == entity_id1), "entity1 should be absent after batch delete");
    assert!(!entities.iter().any(|e| e["id"] == entity_id2), "entity2 should be absent after batch delete");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch delete with an exclusive CSR forwards the operation to the CSR and removes local entities.
#[tokio::test]
#[serial]
async fn distop_batch_delete_exclusive_forwards_to_csr_and_deletes_locally() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-exc-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-exc-2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:batch-delete-exc");

    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&c, &vehicle_entity_bmw(&entity_id2)).await, StatusCode::CREATED);

    let reg = batch_ops_exclusive_speed_registration(&reg_id, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete", 204).await;

    let ids = json!([entity_id1, entity_id2]);
    pretty_assertions::assert_eq!(batch_delete(&c, &ids).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete").await,
        1,
        "exclusive CSR should receive exactly one forwarded batch delete"
    );

    let entities = query_vehicle_entities(&c).await;
    assert!(!entities.iter().any(|e| e["id"] == entity_id1), "entity1 should be absent after batch delete");
    assert!(!entities.iter().any(|e| e["id"] == entity_id2), "entity2 should be absent after batch delete");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Batch delete with two redirect CSRs forwards the operation to both CSR endpoints.
#[tokio::test]
#[serial]
async fn distop_batch_delete_redirect_forwards_to_both_csrs() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:batch-delete-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:batch-delete-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:batch-delete-red-2");

    let reg1 = batch_ops_redirect_registration(&reg_id1, &mock.url("/broker1"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg1).await, StatusCode::CREATED);

    let reg2 = batch_ops_redirect_registration(&reg_id2, &mock.url("/broker2"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg2).await, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete", 204).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/delete", 204).await;

    let ids = json!([entity_id1, entity_id2]);
    pretty_assertions::assert_eq!(batch_delete(&c, &ids).await, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete").await,
        1,
        "broker1 should receive exactly one forwarded batch delete"
    );
    pretty_assertions::assert_eq!(
        mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/delete").await,
        1,
        "broker2 should receive exactly one forwarded batch delete"
    );

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}
