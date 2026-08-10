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

use andromeda_test_fixtures::{context::COMPOUND_CONTEXT, problem_detail as error};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use uuid::Uuid;

const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;
const COMPOUND_CONTEXT_URL: &iri_rs::Iri<&str> = COMPOUND_CONTEXT;

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Database cleanup
// ---------------------------------------------------------------------------

async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

// ---------------------------------------------------------------------------
// Inline fixtures
// ---------------------------------------------------------------------------

use serde_json::json;

/// Basic CSR with multiple information entries (JSON-LD with @context).
fn csr_basic_jsonld(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// Basic CSR without @context (plain JSON).
fn csr_basic_json(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://my.csource.org:1026/"
    })
}

/// CSR with explicit mode and operations.
fn csr_with_mode_and_operations(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "mode": "exclusive",
        "operations": ["queryEntity", "queryTemporal", "updateOps"],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with location geometry.
fn csr_with_location(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "location": {"type": "Point", "coordinates": [-8.521, 41.2]},
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with future expiration date.
fn csr_with_expiration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "expiresAt": "2035-11-29T14:53:15Z",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR without ID (server generates).
fn csr_no_id() -> Value {
    json!({
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with null information (invalid).
fn csr_invalid_null_information(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": null,
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR missing information field (invalid structure).
fn csr_invalid_structure(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with past expiration date.
fn csr_past_expiration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "expiresAt": " 2018-11-29T14:53:15Z",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with detailed information (Building type with propertyNames and relationshipNames).
fn csr_detailed_information(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"type": "Building"}],
                "propertyNames": ["name", "subCategory"],
                "relationshipNames": ["locatedAt"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with only properties information.
fn csr_only_properties(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"propertyNames": ["name", "subCategory"]}],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with custom csource properties.
fn csr_with_csource_properties(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://my.csource.org:1026/",
        "csourceProperty1": "aValue",
        "csourceProperty2": "anotherValue",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with observation interval.
fn csr_with_observation_interval(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "observationInterval": {
            "startAt": "2020-08-01T22:07:00Z",
            "endAt": "2021-08-01T21:07:00Z"
        },
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR with management interval.
fn csr_with_management_interval(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "managementInterval": {
            "startAt": "2020-08-01T22:07:00Z",
            "endAt": "2021-08-01T21:07:00Z"
        },
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// Invalid JSON string (malformed).
const fn csr_invalid_json_string() -> &'static str {
    r#"{"id": "urn:ngsi-ld:ContextSourceRegistration:randomUUID","type": "ContextSourceRegistration",,"endpoint": "http://my.new.csource.org:1026"}"#
}

/// Fragment: endpoint update.
fn fragment_endpoint_update() -> Value {
    json!({"endpoint": "http://my.new.csource.org:1026"})
}

/// Fragment: different type (invalid update).
fn fragment_different_type(id: &str) -> Value {
    json!({
        "id": id,
        "type": "building",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "location": {
            "type": "Polygon",
            "coordinates": [[[100.0, 0.0], [101.0, 0.0], [101.0, 1.0], [100.0, 1.0], [100.0, 0.0]]]
        },
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// Fragment: null expiresAt.
fn fragment_null_expires_at() -> Value {
    json!({"expiresAt": null})
}

/// Expected CSR response in expanded format.
fn expected_csr_expanded(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": "http://my.csource.org:1026/",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "https://ngsi-ld-test-suite/context#Vehicle"}],
                "propertyNames": ["https://ngsi-ld-test-suite/context#brandName", "https://ngsi-ld-test-suite/context#speed"],
                "relationshipNames": ["https://ngsi-ld-test-suite/context#isParked"]
            },
            {
                "entities": [
                    {"type": "https://ngsi-ld-test-suite/context#OffStreetParking", "idPattern": ".*downtown$"},
                    {"type": "https://ngsi-ld-test-suite/context#OffStreetParking", "idPattern": ".*47$"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "mode": "inclusive",
        "operations": ["federationOps"]
    })
}

/// Expected CSR response in compacted format.
fn expected_csr_compacted(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    })
}

/// Expected query result for detailed information.
fn expected_query_detailed(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"type": "Building"}],
                "propertyNames": ["name", "subCategory"],
                "relationshipNames": ["locatedAt"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result for basic CSR (expanded).
fn expected_query_basic_expanded(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            },
            {
                "entities": [
                    {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                    {"idPattern": ".*47$", "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result for only properties.
fn expected_query_only_properties(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"propertyNames": ["name", "subCategory"]}],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result with location.
fn expected_query_with_location(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "location": {"type": "Point", "coordinates": [-8.521, 41.2]},
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result with observation interval.
fn expected_query_with_observation_interval(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "observationInterval": {"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"},
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result with management interval.
fn expected_query_with_management_interval(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "managementInterval": {"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"},
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// Expected query result with csource properties.
fn expected_query_with_csource_properties(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"],
        "csourceProperty1": "aValue",
        "csourceProperty2": "anotherValue"
    }])
}

/// Expected query result for multi-ID query (basic + csource properties).
fn expected_query_multi_ids(id1: &str, id3: &str) -> Value {
    json!([
        {
            "id": id1,
            "type": "ContextSourceRegistration",
            "information": [
                {
                    "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                    "propertyNames": ["brandName", "speed"],
                    "relationshipNames": ["isParked"]
                },
                {
                    "entities": [
                        {"idPattern": ".*downtown$", "type": "OffStreetParking"},
                        {"idPattern": ".*47$", "type": "OffStreetParking"}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
            "endpoint": "http://my.csource.org:1026/",
            "mode": "inclusive",
            "operations": ["federationOps"]
        },
        {
            "id": id3,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://my.csource.org:1026/",
            "mode": "inclusive",
            "operations": ["federationOps"],
            "csourceProperty1": "aValue",
            "csourceProperty2": "anotherValue"
        }
    ])
}

/// Expected query result for detailed information (expanded).
fn expected_query_detailed_expanded(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"type": "https://ngsi-ld-test-suite/context#Building"}],
                "propertyNames": ["https://ngsi-ld-test-suite/context#name", "https://ngsi-ld-test-suite/context#subCategory"],
                "relationshipNames": ["locatedAt"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

// ---------------------------------------------------------------------------
// JSON value helpers
// ---------------------------------------------------------------------------

fn strip_ignored_keys(val: &mut Value, keys: &[&str]) {
    match val {
        Value::Object(map) => {
            for read_only in ["status", "timesSent", "timesFailed", "lastSuccess", "lastFailure"] {
                map.remove(read_only);
            }
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
