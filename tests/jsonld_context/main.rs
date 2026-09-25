//! JSON-LD Context management integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Tests that require a clean database state are annotated with `#[serial]` and call
//! `clean_database()` before running. All tests create and clean up their own resources.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod add;
mod cache_expiration;
mod delete;
mod list;
mod serve;

use andromeda_test_client::broker_endpoint::BrokerEndpoint;
use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail as error};
use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;

const INVALID_REQUEST_TYPE: &iri_rs::Iri<&str> = error::INVALID_REQUEST;
const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;
const LD_CONTEXT_NOT_AVAILABLE_TYPE: &iri_rs::Iri<&str> = error::LD_CONTEXT_NOT_AVAILABLE;

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Private-tenant isolation
// ---------------------------------------------------------------------------

/// A reqwest client that carries `NGSILD-Tenant` on every request, so a test's
/// stored, implicit, and cached `@context` documents live in its own private
/// tenant and its whole-collection `/jsonldContexts` assertions see only what it
/// created. The existing request helpers take a `&Client` unchanged; binding
/// this client is the only per-test change.
fn tenant_client(tenant: &str) -> Client {
    let mut headers = HeaderMap::new();
    headers.insert("ngsild-tenant", HeaderValue::from_str(tenant).expect("tenant name is a valid header value"));
    Client::builder().default_headers(headers).build().expect("tenant client builds")
}

/// Provision the private tenant with a throwaway registration (create then
/// delete) so a read-first test finds an empty but existing tenant rather than
/// `NonexistentTenant`. The registration carries no `@context`, so it creates no
/// implicit context and leaves `/jsonldContexts` empty.
async fn provision_tenant(client: &Client) {
    let registration_id = unique_id("urn:ngsi-ld:ContextSourceRegistration:jsonld-provision");
    let body = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:1",
        "information": [{ "entities": [{ "type": "ProvisioningSentinel" }] }]
    });
    let created = client
        .post(broker_url("csourceRegistrations"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("provisioning registration request failed");
    assert_eq!(
        created.status(),
        StatusCode::CREATED,
        "provisioning registration must be created to bring up the tenant"
    );
    let _ = client.delete(broker_url(&format!("csourceRegistrations/{registration_id}"))).send().await;
}

// ---------------------------------------------------------------------------
// Embedded context server (serves @context documents on ephemeral port)
// ---------------------------------------------------------------------------

struct ContextServer {
    server: JsonLdContextServer,
    urls: HashMap<String, String>,
}

impl ContextServer {
    async fn start(routes: HashMap<String, String>) -> Self {
        let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
            .await
            .expect("JSON-LD context server must start");
        let mut urls = HashMap::new();
        for (path, body) in routes {
            let callback_path = CallbackPath::parse(path.clone()).expect("context route must be absolute");
            let url = server
                .serve(callback_path, Bytes::from(body), HeaderMap::new())
                .await
                .expect("context route must be configured");
            urls.insert(path, url.to_string());
        }
        ContextServer { server, urls }
    }

    /// URL the broker dials to fetch a served `@context`, on whichever host
    /// name reaches this test process.
    fn url(&self, path: &str) -> String {
        self.urls.get(path).expect("configured context route").clone() // clone: callers own callback URL text independently of the server.
    }

    async fn shutdown(self) {
        self.server.shutdown().await.expect("context server shutdown failed");
    }
}

fn broker_url(path: &str) -> String {
    BrokerEndpoint::from_environment()
        .expect("valid broker endpoint")
        .join(path)
        .expect("valid broker-relative test path")
        .to_string()
}

// ---------------------------------------------------------------------------
// Shared helpers for JSON-LD context operations
// ---------------------------------------------------------------------------

/// POST /jsonldContexts, return (status, location, body).
async fn create_context(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let resp = client
        .post(broker_url("jsonldContexts"))
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
        .get(broker_url(&format!("jsonldContexts/{encoded}{sep}{query}")))
        .send()
        .await
        .expect("get_context request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Read `numberOfHits` for a context right now, with no waiting.
///
/// Clause 5.13.3.5 requires the broker to maintain the counter, and the served
/// metadata folds this process's un-flushed accesses into the stored row before
/// answering, so the number is current the instant a request that used the
/// context returns -- a reader never has to wait for the periodic flush.
async fn number_of_hits(client: &Client, ctx_id: &str) -> u64 {
    let (status, body) = get_context(client, ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "metadata read failed: {body:?}");
    body["numberOfHits"].as_u64().unwrap_or(0)
}

/// GET /jsonldContexts with optional query params.
async fn list_contexts(client: &Client, query: &str) -> (StatusCode, Vec<Value>) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(broker_url(&format!("jsonldContexts{sep}{query}")))
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
        .delete(broker_url(&format!("jsonldContexts/{encoded}{sep}{query}")))
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
        .delete(broker_url(&format!("jsonldContexts/{encoded}{sep}{query}")))
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
        .post(broker_url("entities"))
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
    let _ = client.delete(broker_url(&format!("entities/{id}"))).send().await;
}

/// POST /subscriptions with JSON-LD body.
async fn create_subscription(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(broker_url("subscriptions"))
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
    let _ = client.delete(broker_url(&format!("subscriptions/{id}"))).send().await;
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

/// Create a subscription that generates an `ImplicitlyCreated` context. Returns (`sub_id`, `ctx_id`).
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
/// then return (server, `cached_context_url`).
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
    sleep(Duration::from_secs(1)).await;

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
