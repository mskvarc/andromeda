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
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod attribute_details;
mod attribute_info;
mod attributes;
mod entity_type_details;
mod entity_type_info;
mod entity_types;
mod geo_json_accept;

use andromeda_test_fixtures::{context::COMPOUND_CONTEXT, problem_detail as error};
use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use uuid::Uuid;

const COMPOUND_CONTEXT_URL: &iri_rs::Iri<&str> = COMPOUND_CONTEXT;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// Private-tenant isolation
// ---------------------------------------------------------------------------

/// Every discovery test runs against a private tenant (`TenantScopedEnvironment`)
/// so its `/types` and `/attributes` assertions -- which cover collections the
/// API cannot filter -- see only the entities it created. Requests are raw, so
/// the tenant name is carried on the `NGSILD-Tenant` header via `env.tenant()`.
///
/// Provision the private tenant with a throwaway registration (create then
/// delete) so a read-first test (e.g. the empty-collection cases) finds an empty
/// but existing tenant rather than `NonexistentTenant`.
async fn provision_tenant(client: &Client, tenant: &str) {
    let registration_id = unique_id("urn:ngsi-ld:ContextSourceRegistration:discovery-provision");
    let body = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:1",
        "information": [{ "entities": [{ "type": "ProvisioningSentinel" }] }]
    });
    let created = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/json")
        .header("NGSILD-Tenant", tenant)
        .json(&body)
        .send()
        .await
        .expect("provisioning registration request failed");
    assert_eq!(
        created.status(),
        StatusCode::CREATED,
        "provisioning registration must be created to bring up the tenant"
    );
    let _ = client
        .delete(format!("{BASE_URL}/csourceRegistrations/{registration_id}"))
        .header("NGSILD-Tenant", tenant)
        .send()
        .await;
}

// ---------------------------------------------------------------------------
// Inline fixtures
// ---------------------------------------------------------------------------

use serde_json::json;

/// Building entity with simple attributes (name, subCategory, airQualityLevel, almostFull).
fn building_simple_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        }
    })
}

/// Vehicle entity with simple attributes (brandName, isParked relationship).
fn vehicle_simple_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Person:Bob"
            }
        }
    })
}

/// Expected attribute details without context (expanded IRIs).
fn expected_attribute_details_expanded() -> Value {
    json!([
        {
            "id": "https://ngsi-ld-test-suite/context#name",
            "type": "Attribute",
            "attributeName": "https://ngsi-ld-test-suite/context#name",
            "typeNames": ["https://ngsi-ld-test-suite/context#Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#subCategory",
            "type": "Attribute",
            "attributeName": "https://ngsi-ld-test-suite/context#subCategory",
            "typeNames": ["https://ngsi-ld-test-suite/context#Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#airQualityLevel",
            "type": "Attribute",
            "attributeName": "https://ngsi-ld-test-suite/context#airQualityLevel",
            "typeNames": ["https://ngsi-ld-test-suite/context#Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#almostFull",
            "type": "Attribute",
            "attributeName": "https://ngsi-ld-test-suite/context#almostFull",
            "typeNames": ["https://ngsi-ld-test-suite/context#Building"]
        }
    ])
}

/// Expected attribute details with context (compacted names).
fn expected_attribute_details_compacted() -> Value {
    json!([
        {
            "id": "https://ngsi-ld-test-suite/context#name",
            "type": "Attribute",
            "attributeName": "name",
            "typeNames": ["Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#subCategory",
            "type": "Attribute",
            "attributeName": "subCategory",
            "typeNames": ["Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#airQualityLevel",
            "type": "Attribute",
            "attributeName": "airQualityLevel",
            "typeNames": ["Building"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#almostFull",
            "type": "Attribute",
            "attributeName": "almostFull",
            "typeNames": ["Building"]
        }
    ])
}

/// Expected entity type details without context (expanded IRIs).
fn expected_entity_type_details_expanded() -> Value {
    json!([
        {
            "id": "https://ngsi-ld-test-suite/context#Building",
            "type": "EntityType",
            "typeName": "https://ngsi-ld-test-suite/context#Building",
            "attributeNames": [
                "https://ngsi-ld-test-suite/context#name",
                "https://ngsi-ld-test-suite/context#subCategory",
                "https://ngsi-ld-test-suite/context#airQualityLevel",
                "https://ngsi-ld-test-suite/context#almostFull"
            ]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#Vehicle",
            "type": "EntityType",
            "typeName": "https://ngsi-ld-test-suite/context#Vehicle",
            "attributeNames": [
                "https://ngsi-ld-test-suite/context#brandName",
                "https://ngsi-ld-test-suite/context#isParked"
            ]
        }
    ])
}

/// Expected entity type details with context (compacted names).
fn expected_entity_type_details_compacted() -> Value {
    json!([
        {
            "id": "https://ngsi-ld-test-suite/context#Building",
            "type": "EntityType",
            "typeName": "Building",
            "attributeNames": ["name", "subCategory", "airQualityLevel", "almostFull"]
        },
        {
            "id": "https://ngsi-ld-test-suite/context#Vehicle",
            "type": "EntityType",
            "typeName": "Vehicle",
            "attributeNames": ["brandName", "isParked"]
        }
    ])
}

/// Expected attribute list without context (expanded IRIs).
fn expected_attribute_list_expanded() -> Value {
    json!({
        "id": "urn:ngsi-ld:AttributeList:randomUUID",
        "type": "AttributeList",
        "attributeList": [
            "https://ngsi-ld-test-suite/context#airQualityLevel",
            "https://ngsi-ld-test-suite/context#almostFull",
            "https://ngsi-ld-test-suite/context#name",
            "https://ngsi-ld-test-suite/context#subCategory"
        ]
    })
}

/// Expected attribute list with context (compacted names).
fn expected_attribute_list_compacted() -> Value {
    json!({
        "id": "urn:ngsi-ld:AttributeList:randomUUID",
        "type": "AttributeList",
        "attributeList": ["name", "subCategory", "airQualityLevel", "almostFull"]
    })
}

/// Expected entity type list without context (expanded IRIs).
fn expected_entity_type_list_expanded() -> Value {
    json!({
        "id": "urn:ngsi-ld:EntityTypeList:randomUUID",
        "type": "EntityTypeList",
        "typeList": [
            "https://ngsi-ld-test-suite/context#Building",
            "https://ngsi-ld-test-suite/context#Vehicle"
        ]
    })
}

/// Expected entity type list with context (compacted names).
fn expected_entity_type_list_compacted() -> Value {
    json!({
        "id": "urn:ngsi-ld:EntityTypeList:randomUUID",
        "type": "EntityTypeList",
        "typeList": ["Building", "Vehicle"]
    })
}

/// Expected attribute info for airQualityLevel.
fn expected_attribute_info() -> Value {
    json!({
        "id": "https://ngsi-ld-test-suite/context#airQualityLevel",
        "type": "Attribute",
        "attributeName": "airQualityLevel",
        "attributeCount": 1,
        "attributeTypes": ["Property"],
        "typeNames": ["Building"]
    })
}

/// Expected entity type info for Building.
fn expected_entity_type_info() -> Value {
    json!({
        "id": "https://ngsi-ld-test-suite/context#Building",
        "type": "EntityTypeInfo",
        "typeName": "Building",
        "entityCount": 2,
        "attributeDetails": [
            {
                "id": "https://ngsi-ld-test-suite/context#name",
                "type": "Attribute",
                "attributeName": "name",
                "attributeTypes": ["Property"]
            },
            {
                "id": "https://ngsi-ld-test-suite/context#subCategory",
                "type": "Attribute",
                "attributeName": "subCategory",
                "attributeTypes": ["Property"]
            },
            {
                "id": "https://ngsi-ld-test-suite/context#airQualityLevel",
                "type": "Attribute",
                "attributeName": "airQualityLevel",
                "attributeTypes": ["Property"]
            },
            {
                "id": "https://ngsi-ld-test-suite/context#almostFull",
                "type": "Attribute",
                "attributeName": "almostFull",
                "attributeTypes": ["Property"]
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn link_header(ctx_url: &str) -> String {
    format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// POST /entities with `application/json` body and optional Link context. Returns (status, location).
async fn create_entity(client: &Client, tenant: &str, body: &Value, link_ctx: Option<&str>) -> (StatusCode, Option<String>) {
    let mut req = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .header("NGSILD-Tenant", tenant)
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
async fn delete_entity(client: &Client, tenant: &str, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{id}")).header("NGSILD-Tenant", tenant).send().await;
}

/// GET /types with optional query string and Link context. Returns (status, body).
async fn list_entity_types(client: &Client, tenant: &str, query: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/types{sep}{query}")).header("NGSILD-Tenant", tenant);
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("list_entity_types request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /`types/{type_name`} with optional Link context. Returns (status, body).
async fn retrieve_entity_type(client: &Client, tenant: &str, type_name: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let mut req = client.get(format!("{BASE_URL}/types/{type_name}")).header("NGSILD-Tenant", tenant);
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("retrieve_entity_type request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /attributes with optional query string and Link context. Returns (status, body).
async fn list_attributes(client: &Client, tenant: &str, query: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/attributes{sep}{query}")).header("NGSILD-Tenant", tenant);
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("list_attributes request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /`attributes/{attr_name`} with optional Link context. Returns (status, body).
async fn retrieve_attribute(client: &Client, tenant: &str, attr_name: &str, link_ctx: Option<&str>) -> (StatusCode, Value) {
    let mut req = client.get(format!("{BASE_URL}/attributes/{attr_name}")).header("NGSILD-Tenant", tenant);
    if let Some(ctx_url) = link_ctx {
        req = req.header("Link", link_header(ctx_url));
    }
    let resp = req.send().await.expect("retrieve_attribute request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}
