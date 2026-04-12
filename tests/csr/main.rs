//! Context Source Registration (CSR) integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Tests that require a clean database state are annotated with `#[serial]` and call
//! `clean_database()` before running. All tests create and clean up their own resources.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod create;
mod delete;
mod query;
mod retrieve;
mod update;

use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const BAD_REQUEST_DATA_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
const RESOURCE_NOT_FOUND_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
const INVALID_REQUEST_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";

const COMPOUND_CONTEXT_URL: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Database cleanup
// ---------------------------------------------------------------------------

const TRUNCATE_REGISTRATIONS: &str = "TRUNCATE context_source_registrations CASCADE";

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_REGISTRATIONS)
        .execute(&pool)
        .await
        .expect("failed to truncate registrations");
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

/// Load a fixture, replace the ID placeholder, keep original @context URLs.
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

/// Load a fragment (no ID replacement).
fn load_fragment(path: &str) -> Value {
    let contents = load_raw(path);
    serde_json::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse fragment {path}: {e}"))
}

/// Load an expectation fixture for response comparison.
fn load_expectation(path: &str) -> Value {
    let contents = load_raw(path);
    serde_json::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse expectation {path}: {e}"))
}

/// Load a raw fixture file as a string (for malformed JSON tests).
fn load_fixture_raw(path: &str) -> String {
    load_raw(path)
}

// ---------------------------------------------------------------------------
// JSON value helpers
// ---------------------------------------------------------------------------

fn strip_ignored_keys(val: &mut Value, keys: &[&str]) {
    match val {
        Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for v in map.values_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// POST /csourceRegistrations with JSON-LD body. Returns (status, location, body).
async fn create_registration(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let resp = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_registration request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    let body: Value = resp.json().await.unwrap_or_default();
    (status, location, body)
}

/// GET /csourceRegistrations/{id} with optional headers. Returns (status, body).
async fn retrieve_registration(client: &Client, id: &str, accept: Option<&str>, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let mut req = client.get(format!("{BASE_URL}/csourceRegistrations/{id}"));
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link);
    }
    let resp = req.send().await.expect("retrieve_registration request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// DELETE /csourceRegistrations/{id}. Returns (status, body).
async fn delete_registration(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .delete(format!("{BASE_URL}/csourceRegistrations/{id}"))
        .send()
        .await
        .expect("delete_registration request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// PATCH /csourceRegistrations/{id} with given content type. Returns (status, body).
async fn update_registration(client: &Client, id: &str, body: &Value, content_type: &str) -> (StatusCode, Value) {
    let resp = client
        .patch(format!("{BASE_URL}/csourceRegistrations/{id}"))
        .header("Content-Type", content_type)
        .json(body)
        .send()
        .await
        .expect("update_registration request failed");
    let status = resp.status();
    let resp_body: Value = resp.json().await.unwrap_or_default();
    (status, resp_body)
}

/// PATCH /csourceRegistrations/{id} with raw bytes. Returns (status, body).
async fn update_registration_raw(client: &Client, id: &str, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let resp = client
        .patch(format!("{BASE_URL}/csourceRegistrations/{id}"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("update_registration_raw request failed");
    let status = resp.status();
    let resp_body: Value = resp.json().await.unwrap_or_default();
    (status, resp_body)
}

/// Generic request helper for custom method/path/body/headers combinations.
async fn request(
    client: &Client,
    method: Method,
    path: &str,
    body: Option<&Value>,
    content_type: Option<&str>,
    link_ctx: Option<&str>,
    accept: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = client.request(method, format!("{BASE_URL}{path}"));
    if let Some(ct) = content_type {
        req = req.header("Content-Type", ct);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link);
    }
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(body) = body {
        req = req.json(body);
    }
    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let resp_body: Value = resp.json().await.unwrap_or_default();
    (status, resp_body)
}

/// GET /csourceRegistrations with query string. Returns (status, body).
async fn query_registrations(client: &Client, query: &str, accept: Option<&str>, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/csourceRegistrations{sep}{query}"));
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link);
    }
    let resp = req.send().await.expect("query_registrations request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Cleanup: delete a registration, ignoring errors.
async fn cleanup_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{id}")).send().await;
}
