#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Common helpers for distributed entity operation tests.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use urlencoding::encode;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test utilities
// ---------------------------------------------------------------------------

pub fn client() -> Client {
    Client::new()
}

pub fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

pub fn jsonld_link_value() -> String {
    format!("<{COMPOUND_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

pub async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

// ---------------------------------------------------------------------------
// Entity builders
// ---------------------------------------------------------------------------

pub fn vehicle_entity(id: &str) -> Value {
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
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_entity_with_speed(id: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": speed
        },
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_entity_speed_only(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_entity_second(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "BMW"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        }
    })
}

pub fn vehicle_entity_second_ld(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "BMW"
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_entity_different_ld(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown3",
            "observedAt": "2018-07-29T12:00:04Z"
        },
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_entity_with_scope(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50
        },
        "scope": scope,
        "@context": COMPOUND_CONTEXT
    })
}

/// Vehicle with brandName and isParked (no context, for JSON content type).
pub fn vehicle_entity_json(id: &str) -> Value {
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
            "observedAt": "2017-07-29T12:00:04Z"
        }
    })
}

/// Replacement payload with different attributes (no context, JSON).
pub fn vehicle_replacement_json(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 120
        },
        "fuelType": {
            "type": "Property",
            "value": "diesel"
        }
    })
}

/// A Building carrying nothing but its identity, used as the body a context
/// source answers a Linked Entity retrieval with.
pub fn minimal_building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "category": {
            "type": "Property",
            "value": "commercial"
        },
        "@context": COMPOUND_CONTEXT
    })
}

/// A Building whose `linkedTo` Relationship names `target`.
///
/// Clause 4.5.23.1 makes the `objectType` annotation the eligibility gate for
/// resolving the object outside the local store: with it the object is an
/// Internal Linked Entity a registered context source may hold, without it an
/// object the store does not have stays a plain URI.
pub fn building_linking_to(id: &str, target: &str, object_type: Option<&str>) -> Value {
    let mut entity = json!({
        "id": id,
        "type": "Building",
        "linkedTo": {
            "type": "Relationship",
            "object": target
        },
        "@context": COMPOUND_CONTEXT
    });

    if let Some(object_type) = object_type {
        entity["linkedTo"]["objectType"] = json!(object_type);
    }

    entity
}

// ---------------------------------------------------------------------------
// Registration builders
// ---------------------------------------------------------------------------

/// Registration of one Building held by the context source at `endpoint`.
pub fn building_registration(reg_id: &str, entity_id: &str, endpoint: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Building"
                    }
                ]
            }
        ],
        "endpoint": endpoint,
        "@context": COMPOUND_CONTEXT
    })
}

pub fn vehicle_registration(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
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
        "@context": COMPOUND_CONTEXT
    });

    if let Some(eid) = entity_id {
        reg["information"][0]["entities"][0]["id"] = json!(eid);
    }
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

pub fn vehicle_registration_with_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>, operations: &[&str]) -> Value {
    let mut reg = vehicle_registration(reg_id, entity_id, endpoint, mode);
    reg["operations"] = json!(operations);
    reg
}

pub fn vehicle_registration_with_alias(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>, alias: &str) -> Value {
    let mut reg = vehicle_registration(reg_id, entity_id, endpoint, mode);
    reg["contextSourceAlias"] = json!(alias);
    reg
}

pub fn vehicle_registration_speed_property(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>, operations: Option<&[&str]>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "@context": COMPOUND_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }
    if let Some(ops) = operations {
        reg["operations"] = json!(ops);
    }

    reg
}

/// Vehicle registration with redirection operations (createEntity, updateEntity, deleteEntity, mergeEntity, replaceEntity).
pub fn vehicle_registration_redirection_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = ["createEntity", "updateEntity", "deleteEntity", "mergeEntity", "replaceEntity"];
    let mut reg = json!({
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
        "operations": ops,
        "@context": COMPOUND_CONTEXT
    });

    if let Some(eid) = entity_id {
        reg["information"][0]["entities"][0]["id"] = json!(eid);
    }
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

/// Speed-specific registration with redirection operations.
pub fn vehicle_registration_speed_redirection_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = ["createEntity", "updateEntity", "deleteEntity", "mergeEntity", "replaceEntity"];
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "operations": ops,
        "@context": COMPOUND_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

/// Speed-specific registration with default operations (no redirection ops).
pub fn vehicle_registration_speed_default_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ],
                "propertyNames": ["speed"]
            }
        ],
        "endpoint": endpoint,
        "@context": COMPOUND_CONTEXT
    });

    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }

    reg
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

pub async fn create_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

pub async fn create_entity_local(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities?local=true"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity_local request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

pub async fn create_registration(client: &Client, reg: &Value) -> (StatusCode, Option<String>) {
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

pub async fn get_entity(client: &Client, id: &str, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities/{}{sep}{query}", encode(id)))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities(client: &Client, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities_with_via(client: &Client, query: &str, link: &str, via: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link)
        .header("Via", via)
        .send()
        .await
        .expect("query_entities_with_via request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn query_entities_post(client: &Client, body: &Value, link: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/query"))
        .header("Accept", "application/ld+json")
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(body)
        .send()
        .await
        .expect("query_entities_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

pub async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

pub async fn delete_entity_with_status(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}", encode(id)))
        .send()
        .await
        .expect("delete_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn delete_entity_local(client: &Client, id: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}?local=true", encode(id)))
        .send()
        .await
        .expect("delete_entity_local request failed");
    resp.status()
}

pub async fn merge_entity(client: &Client, id: &str, payload: &Value, link: &str) -> (StatusCode, Value) {
    // Strip @context from body - application/json must not contain @context per NGSI-LD 6.3.5
    let mut body = payload.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.remove("@context");
    }
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}", encode(id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(&body)
        .send()
        .await
        .expect("merge_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn replace_entity(client: &Client, id: &str, payload: &Value, link: &str) -> (StatusCode, Value) {
    let resp = client
        .put(format!("{BASE_URL}/entities/{}", encode(id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(payload)
        .send()
        .await
        .expect("replace_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

pub async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// Warning header extraction
// ---------------------------------------------------------------------------

pub fn warning_header_values(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect()
}
