//! Context Source Registration (CSR) integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Tests that require a clean database state are annotated with `#[serial]` and call
//! `clean_database()` before running. All tests create and clean up their own resources.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod create;
mod delete;
mod query;
mod retrieve;
mod routing_projection;
mod update;

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::COMPOUND_CONTEXT, problem_detail as error};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use uuid::Uuid;

const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;
const LD_CONTEXT_NOT_AVAILABLE_TYPE: &iri_rs::Iri<&str> = error::LD_CONTEXT_NOT_AVAILABLE;

/// A remote `@context` at an address nothing answers on, so its resolution
/// fails with a connection refusal rather than a slow timeout (clause 5.5.6).
const UNREACHABLE_CONTEXT: &str = "http://127.0.0.1:9/unreachable.jsonld";
const COMPOUND_CONTEXT_URL: &iri_rs::Iri<&str> = COMPOUND_CONTEXT;

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

/// Force the test's private tenant into existence, then leave its collection
/// empty. The broker provisions a tenant's database only on the first write, so
/// a test whose first request is a read or a validation error would otherwise
/// see a `NonexistentTenant` error instead of the `ResourceNotFound` or
/// `BadRequestData` it asserts. Creating and immediately deleting one
/// registration provisions the tenant without leaving anything a later query or
/// whole-collection comparison would observe.
async fn provision_tenant(client: &TestClient) {
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:tenant-provision");
    let (status, _, _) = create_registration(client, &csr_basic_jsonld(&id)).await;
    assert_eq!(status, StatusCode::CREATED, "tenant provisioning registration must be created");
    cleanup_registration(client, &id).await;
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
///
/// Clause 4.3.6.2: "An exclusive registration shall always relate to specific
/// Attributes found on a single Entity. Thus, the registration shall define
/// both: An entity id (i.e. an id pattern or Entity type defining a group of
/// entities is not supported for exclusive registrations). Attributes." The
/// entity is derived from the registration identifier so that each run pins a
/// fresh entity and never trips the clause 5.9.2.4 overlap Conflict.
fn csr_with_mode_and_operations(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": id.replace("ContextSourceRegistration", "Vehicle"), "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
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

/// The `Vehicle` entity an entity-id discovery asks for.
const REGISTERED_VEHICLE_ID: &str = "urn:ngsi-ld:Vehicle:A456";

/// A `Vehicle` entity no entity-id discovery in this suite asks for.
const UNREGISTERED_VEHICLE_ID: &str = "urn:ngsi-ld:Vehicle:B789";

/// CSR advertising `speed` on one named `Vehicle` entity.
fn csr_vehicle_by_entity_id(id: &str, entity_id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"id": entity_id, "type": "Vehicle"}], "propertyNames": ["speed"]}],
        "endpoint": "http://my.csource.org:1026/",
        "@context": [COMPOUND_CONTEXT_URL]
    })
}

/// CSR advertising `speed` on every `Vehicle` entity, naming none of them.
fn csr_vehicle_by_type(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]}],
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

/// Expected query result for the basic CSR when one of its `RegistrationInfo`
/// elements matches.
///
/// Clause 5.10.2.5 only recommends the narrower answer -- implementations
/// "should return filtered Context Source Registrations, which only contain
/// context source registration information relevant for the query" -- so a
/// registration that matches is returned whole: a discovery client asks what
/// a source can provide, not which part of it matched the question it happened
/// to ask. A Context Source Notification is the other case (clause 5.11.7) and
/// does trim, because there the subscriber is told what changed.
/// The `csr_basic_jsonld` registration as a discovery by `type=Vehicle` returns
/// it: clause 5.10.2.5 recommends the narrowed answer, so only the Vehicle
/// `RegistrationInfo` element survives and the `OffStreetParking` element is
/// trimmed away.
fn expected_query_vehicle_information(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            }
        ],
        "endpoint": "http://my.csource.org:1026/",
        "mode": "inclusive",
        "operations": ["federationOps"]
    }])
}

/// The `csr_basic_jsonld` registration as a discovery by `type=OffStreetParking`
/// returns it: clause 5.10.2.5 trims to the matching `RegistrationInfo`
/// element, so only the `OffStreetParking` element survives -- whole, both of its
/// `idPattern` entities included, because the trim is at element granularity --
/// and the Vehicle element is dropped.
fn expected_query_offstreetparking_information(id: &str) -> Value {
    json!([{
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
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

/// Expected query result for an entity-id query: the registration pinned to the
/// requested entity, and the one registering the whole type.
///
/// Clause 5.12: an `EntityInfo` matches when "One of the specified Entity
/// identifiers matches the id in the `EntityInfo`" and, independently, when "The
/// `EntityInfo` contains neither an id nor an idPattern".
fn expected_query_matching_entity_ids(pinned_id: &str, type_wide_id: &str) -> Value {
    json!([
        {
            "id": pinned_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"id": REGISTERED_VEHICLE_ID, "type": "Vehicle"}], "propertyNames": ["speed"]}],
            "endpoint": "http://my.csource.org:1026/",
            "mode": "inclusive",
            "operations": ["federationOps"]
        },
        {
            "id": type_wide_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]}],
            "endpoint": "http://my.csource.org:1026/",
            "mode": "inclusive",
            "operations": ["federationOps"]
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
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// POST /csourceRegistrations with JSON-LD body. Returns (status, location, body).
async fn create_registration(client: &TestClient, body: &Value) -> (StatusCode, Option<String>, Value) {
    let resp = client
        .raw(Method::POST, "/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_registration request failed");
    let location = resp.location().expect("valid location header").map(ToOwned::to_owned);
    (resp.status, location, resp.json_value().unwrap_or_default())
}

/// GET /csourceRegistrations/{id} with optional headers. Returns (status, body).
async fn retrieve_registration(client: &TestClient, id: &str, accept: Option<&str>, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let path = format!("/ngsi-ld/v1/csourceRegistrations/{id}");
    let mut req = client.raw(Method::GET, &path);
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link.as_str());
    }
    let resp = req.send().await.expect("retrieve_registration request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// DELETE /csourceRegistrations/{id}. Returns (status, body).
async fn delete_registration(client: &TestClient, id: &str) -> (StatusCode, Value) {
    let resp = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
        .send()
        .await
        .expect("delete_registration request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// PATCH /csourceRegistrations/{id} with given content type. Returns (status, body).
async fn update_registration(client: &TestClient, id: &str, body: &Value, content_type: &str) -> (StatusCode, Value) {
    let resp = client
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
        .header("Content-Type", content_type)
        .json(body)
        .send()
        .await
        .expect("update_registration request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// PATCH /csourceRegistrations/{id} with raw bytes. Returns (status, body).
async fn update_registration_raw(client: &TestClient, id: &str, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let resp = client
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("update_registration_raw request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// Generic request helper for custom method/path/body/headers combinations.
async fn request(
    client: &TestClient,
    method: Method,
    path: &str,
    body: Option<&Value>,
    content_type: Option<&str>,
    link_ctx: Option<&str>,
    accept: Option<&str>,
) -> (StatusCode, Value) {
    let full_path = format!("/ngsi-ld/v1{path}");
    let mut req = client.raw(method, &full_path);
    if let Some(ct) = content_type {
        req = req.header("Content-Type", ct);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link.as_str());
    }
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(body) = body {
        req = req.json(body);
    }
    let resp = req.send().await.expect("request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// GET /csourceRegistrations with query string. Returns (status, body).
async fn query_registrations(client: &TestClient, query: &str, accept: Option<&str>, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let path = format!("/ngsi-ld/v1/csourceRegistrations{sep}{query}");
    let mut req = client.raw(Method::GET, &path);
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    if let Some(ctx_url) = link_ctx {
        let link = format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        req = req.header("Link", link.as_str());
    }
    let resp = req.send().await.expect("query_registrations request failed");
    (resp.status, resp.json_value().unwrap_or_default())
}

/// Cleanup: delete a registration, ignoring errors.
async fn cleanup_registration(client: &TestClient, id: &str) {
    let _ = client.raw(Method::DELETE, &format!("/ngsi-ld/v1/csourceRegistrations/{id}")).send().await;
}
