//! NGSI-LD discovery (types and attributes) integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Tests that require a clean database state are annotated with `#[serial]` and call
//! `clean_database()` before running. All tests create and clean up their own resources.
//!
//! Naming convention: `<operation>_<scope>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod attribute_details;
mod attribute_info;
mod attributes;
mod entity_type_details;
mod entity_type_info;
mod entity_types;

use reqwest::{Client, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const COMPOUND_CONTEXT_URL: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";
const RESOURCE_NOT_FOUND_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Database cleanup
// ---------------------------------------------------------------------------

const TRUNCATE_ENTITIES: &str = "TRUNCATE entities CASCADE";

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ENTITIES).execute(&pool).await.expect("failed to truncate entities");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_raw(path: &str) -> String {
    let full_path = fixtures_dir().join(path);
    std::fs::read_to_string(&full_path).unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", full_path.display()))
}

/// Load a fixture JSON file and replace the `id` field with the given value.
fn load_fixture(path: &str, id: &str) -> Value {
    let contents = load_raw(path);
    let mut value: Value = serde_json::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"));
    if let Some(obj) = value.as_object_mut() {
        if obj.contains_key("id") {
            obj.insert("id".to_string(), Value::String(id.to_string()));
        }
    }
    value
}

/// Load an expectation fixture for response comparison.
fn load_expectation(path: &str) -> Value {
    let contents = load_raw(path);
    serde_json::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse expectation {path}: {e}"))
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn link_header(ctx_url: &str) -> String {
    format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// POST /entities with `application/json` body and optional Link context. Returns (status, location).
async fn create_entity(client: &Client, body: &Value, link_ctx: Option<&str>) -> (StatusCode, Option<String>) {
    let mut req = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .json(body);
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// DELETE /entities/{id}.
async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{id}")).send().await;
}

/// GET /types with optional query string and Link context. Returns (status, body).
async fn list_entity_types(client: &Client, query: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/types{sep}{query}"));
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("list_entity_types request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /types/{type_name} with optional Link context. Returns (status, body).
async fn retrieve_entity_type(client: &Client, type_name: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let mut req = client.get(format!("{BASE_URL}/types/{type_name}"));
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("retrieve_entity_type request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /attributes with optional query string and Link context. Returns (status, body).
async fn list_attributes(client: &Client, query: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/attributes{sep}{query}"));
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("list_attributes request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /attributes/{attr_name} with optional Link context. Returns (status, body).
async fn retrieve_attribute(client: &Client, attr_name: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let mut req = client.get(format!("{BASE_URL}/attributes/{attr_name}"));
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("retrieve_attribute request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}
