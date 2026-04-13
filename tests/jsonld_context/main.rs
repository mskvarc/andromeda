//! JSON-LD Context management integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Tests that require a clean database state are annotated with `#[serial]` and call
//! `clean_database()` before running. All tests create and clean up their own resources.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod add;
mod delete;
mod list;
mod serve;

use axum::response::IntoResponse;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const INVALID_REQUEST_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
const BAD_REQUEST_DATA_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
const RESOURCE_NOT_FOUND_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
const LD_CONTEXT_NOT_AVAILABLE_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Database cleanup
// ---------------------------------------------------------------------------

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

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Embedded context server (serves @context documents on ephemeral port)
// ---------------------------------------------------------------------------

struct ContextServer {
    port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ContextServer {
    async fn start(routes: HashMap<String, String>) -> Self {
        let routes = Arc::new(RwLock::new(routes));
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let store = routes.clone();
            async move {
                let path = req.uri().path().to_string();
                let routes = store.read().await;
                if let Some(body) = routes.get(&path) {
                    (
                        axum::http::StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, "application/ld+json")],
                        body.clone(),
                    )
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

        Self {
            port,
            shutdown: Some(shutdown_tx),
        }
    }

    /// URL using host.docker.internal so the broker (in Docker) can reach us.
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
// Shared helpers for JSON-LD context operations
// ---------------------------------------------------------------------------

/// POST /jsonldContexts, return (status, location, body).
async fn create_context(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let resp = client
        .post(format!("{BASE_URL}/jsonldContexts"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("create_context request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    let body: Value = resp.json().await.unwrap_or_default();
    (status, location, body)
}

/// GET /jsonldContexts/{id} with optional query params.
async fn get_context(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/jsonldContexts/{encoded}{sep}{query}"))
        .send()
        .await
        .expect("get_context request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /jsonldContexts with optional query params.
async fn list_contexts(client: &Client, query: &str) -> (StatusCode, Vec<Value>) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/jsonldContexts{sep}{query}"))
        .send()
        .await
        .expect("list_contexts request failed");
    let status = resp.status();
    let body: Vec<Value> = resp.json().await.unwrap_or_default();
    (status, body)
}

/// DELETE /jsonldContexts/{id} with optional query params.
async fn delete_context(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .delete(format!("{BASE_URL}/jsonldContexts/{encoded}{sep}{query}"))
        .send()
        .await
        .expect("delete_context request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// DELETE /jsonldContexts/{id}, return only status + raw bytes (for empty body checks).
async fn delete_context_raw(client: &Client, id: &str, query: &str) -> (StatusCode, Vec<u8>) {
    let encoded = urlencoding::encode(id);
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .delete(format!("{BASE_URL}/jsonldContexts/{encoded}{sep}{query}"))
        .send()
        .await
        .expect("delete_context_raw request failed");
    let status = resp.status();
    let bytes = resp.bytes().await.unwrap_or_default().to_vec();
    (status, bytes)
}

/// POST /entities with JSON-LD body.
async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// DELETE /entities/{id}.
async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{id}")).send().await;
}

/// POST /subscriptions with JSON-LD body.
async fn create_subscription(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_subscription request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// DELETE /subscriptions/{id}.
async fn delete_subscription(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/subscriptions/{id}")).send().await;
}

/// Extract the last path segment (ID) from a Location header or URI string.
fn extract_id(uri: &str) -> String {
    uri.rsplit('/').next().unwrap().to_string()
}

/// Create a hosted context from a key-value dict body and return its ID.
async fn create_hosted_context(client: &Client) -> String {
    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, location, _) = create_context(client, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    extract_id(location.as_ref().expect("should have Location"))
}

/// Create a subscription that generates an ImplicitlyCreated context. Returns (sub_id, ctx_id).
async fn create_implicit_context(client: &Client) -> (String, String) {
    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {
            "endpoint": {"uri": "http://a.b.c/123"}
        },
        "@context": [
            "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
            "http://smartdatamodels.org/context.jsonld"
        ]
    });
    let (status, location) = create_subscription(client, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let sub_id = extract_id(location.as_ref().unwrap());

    // Find the implicitly created context
    let (status, contexts) = list_contexts(client, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ctx_uri = contexts
        .first()
        .expect("should have implicit context")
        .as_str()
        .expect("context should be a string URI");
    let ctx_id = extract_id(ctx_uri);

    (sub_id, ctx_id)
}

/// Start a local context server, create an entity that references it (causing caching),
/// then return (server, cached_context_url).
async fn setup_cached_context(client: &Client) -> (ContextServer, String) {
    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    let entity_id = unique_id("urn:ngsi-ld:Testing:cached");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(client, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Allow time for caching
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    (server, ctx_url)
}

/// Find a cached context by its URL, returning its localId.
async fn find_cached_context_id(client: &Client, ctx_url: &str) -> String {
    let (status, contexts) = list_contexts(client, "kind=Cached&details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let cached_ctx = contexts
        .iter()
        .find(|c| c.get("URL").and_then(|u| u.as_str()).is_some_and(|u| u == ctx_url))
        .expect("context should be cached");
    cached_ctx["localId"].as_str().unwrap().to_string()
}
