#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Common helpers for distributed entity attribute operation tests.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use urlencoding::encode;

// ---------------------------------------------------------------------------
// Test utilities
// ---------------------------------------------------------------------------

pub fn client() -> Client {
    Client::new()
}

pub fn jsonld_link_value() -> String {
    format!("<{COMPOUND_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

// ---------------------------------------------------------------------------
// Entity builders
// ---------------------------------------------------------------------------

/// Vehicle entity with brandName and isParked attributes.
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

/// Vehicle entity with speed and isParked2 attributes (different from standard).
pub fn vehicle_entity_different(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56
        },
        "isParked2": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": COMPOUND_CONTEXT
    })
}

/// Vehicle entity with two datasetId speed attributes (for delete tests).
pub fn vehicle_entity_two_speed_datasets(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "speed": [
            {
                "type": "Property",
                "value": 50,
                "datasetId": "urn:ngsi-ld:Dataset:speed:1"
            },
            {
                "type": "Property",
                "value": 60
            }
        ],
        "@context": COMPOUND_CONTEXT
    })
}

// ---------------------------------------------------------------------------
// Fragment builders (attribute-level payloads)
// ---------------------------------------------------------------------------

/// Fragment with speed and isParked for append operations.
pub fn fragment_speed_isparked() -> Value {
    json!({
        "speed": {
            "type": "Property",
            "value": 120
        },
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown2",
            "observedAt": "2018-01-01T12:00:00Z"
        }
    })
}

/// Fragment with speed for update/partial update operations.
pub fn fragment_speed_different() -> Value {
    json!({
        "speed": {
            "type": "Property",
            "value": 200
        }
    })
}

/// Fragment with speed with datasetId for partial update operations.
pub fn fragment_speed_datasetid() -> Value {
    json!({
        "type": "Property",
        "value": 150,
        "datasetId": "urn:ngsi-ld:Dataset:speed:updated"
    })
}

/// Bare `speed` attribute fragment without a datasetId, targeting the Entity's
/// existing default-dataset instance. A Partial Update Attribute selects the
/// instance by datasetId (clause 5.6.4), so a fragment naming a datasetId the
/// Entity does not hold updates nothing and answers 404 when the operation is
/// not forwarded.
pub fn fragment_speed_no_datasetid() -> Value {
    json!({
        "type": "Property",
        "value": 150
    })
}

/// Fragment with brandName for update operations.
pub fn fragment_brandname() -> Value {
    json!({
        "brandName": {
            "type": "Property",
            "value": "BMW"
        }
    })
}

// ---------------------------------------------------------------------------
// Registration builders
// ---------------------------------------------------------------------------

/// Vehicle registration with entity and attribute redirection operations.
///
/// Includes both entity-level operations (createEntity, updateEntity, etc.) and
/// attribute-level operations (appendAttrs, updateAttrs, deleteAttrs).
pub fn vehicle_registration_redirection_ops(reg_id: &str, entity_id: Option<&str>, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = [
        "createEntity",
        "updateEntity",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "appendAttrs",
        "updateAttrs",
        "deleteAttrs",
    ];
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

/// Speed-specific registration with entity and attribute redirection operations.
///
/// Includes both entity-level and attribute-level operations for the speed property.
pub fn vehicle_registration_speed_redirection_ops(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let ops = [
        "createEntity",
        "updateEntity",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "appendAttrs",
        "updateAttrs",
        "deleteAttrs",
    ];
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

/// Auxiliary Vehicle registration for a single entity, advertising retrieval only.
///
/// Clause 5.9.2.4 admits only `retrieveOps`, `retrieveEntity` and `queryEntity`
/// on an auxiliary registration, so the provision operations advertised by
/// `vehicle_registration_redirection_ops` cannot be carried here.
pub fn vehicle_registration_auxiliary_retrieve_ops(reg_id: &str, entity_id: &str, endpoint: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "id": entity_id,
                        "type": "Vehicle"
                    }
                ]
            }
        ],
        "endpoint": endpoint,
        "mode": "auxiliary",
        "operations": ["retrieveOps"],
        "@context": COMPOUND_CONTEXT
    })
}

// ---------------------------------------------------------------------------
// HTTP helpers - Entity operations
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

pub async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

// ---------------------------------------------------------------------------
// HTTP helpers - Entity attribute operations
// ---------------------------------------------------------------------------

/// Append entity attributes (POST /entities/{entityId}/attrs).
pub async fn append_entity_attrs(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities/{}/attrs", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("append_entity_attrs request failed");
    resp.status()
}

/// Append entity attributes with noOverwrite option.
pub async fn append_entity_attrs_no_overwrite(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities/{}/attrs?options=noOverwrite", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("append_entity_attrs_no_overwrite request failed");
    resp.status()
}

/// Update entity attributes (PATCH /entities/{entityId}/attrs).
pub async fn update_entity_attrs(client: &Client, entity_id: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}/attrs", encode(entity_id)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("update_entity_attrs request failed");
    resp.status()
}

/// Partial update of entity attribute (PATCH /entities/{entityId}/attrs/{attrId}).
pub async fn partial_update_attr(client: &Client, entity_id: &str, attr_name: &str, fragment: &Value, link: &str) -> StatusCode {
    let resp = client
        .patch(format!("{BASE_URL}/entities/{}/attrs/{}", encode(entity_id), encode(attr_name)))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(fragment)
        .send()
        .await
        .expect("partial_update_attr request failed");
    resp.status()
}

/// Delete entity attribute (DELETE /entities/{entityId}/attrs/{attrId}).
pub async fn delete_entity_attr(client: &Client, entity_id: &str, attr_name: &str, link: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}/attrs/{}", encode(entity_id), encode(attr_name)))
        .header("Link", link)
        .send()
        .await
        .expect("delete_entity_attr request failed");
    resp.status()
}

/// Delete entity attribute with deleteAll=true option.
pub async fn delete_entity_attr_all(client: &Client, entity_id: &str, attr_name: &str, link: &str) -> StatusCode {
    let resp = client
        .delete(format!("{BASE_URL}/entities/{}/attrs/{}?deleteAll=true", encode(entity_id), encode(attr_name)))
        .header("Link", link)
        .send()
        .await
        .expect("delete_entity_attr_all request failed");
    resp.status()
}

// ---------------------------------------------------------------------------
// HTTP helpers - Registration operations
// ---------------------------------------------------------------------------

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

pub async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}
