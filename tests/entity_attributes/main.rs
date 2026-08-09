//! Entity attribute provision integration tests for the NGSI-LD context broker.
//!
//! Tests entity attribute operations:
//! - Append: add new attributes to existing entities
//! - Update: update existing attributes (PATCH /entities/{id}/attrs)
//! - Partial update: update specific attribute (PATCH /entities/{id}/attrs/{attrId})
//! - Replace: replace specific attribute (PUT /entities/{id}/attrs/{attrId})
//! - Delete: remove attributes from entities
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `attribute_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod append;
mod delete;
mod partial_update;
mod replace;
mod update;

use andromeda_test_common::{
    config::{BASE_URL, COMPOUND_CONTEXT, DATABASE_URL, TEST_SUITE_CONTEXT},
    db::TRUNCATE_ALL,
    header::jsonld_link_header,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

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
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Entity factories
// ---------------------------------------------------------------------------

fn building_minimal(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_observation_space(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "observationSpace": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.35, 52.51], [13.40, 52.51], [13.40, 52.56], [13.35, 52.56], [13.35, 52.51]]]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_operation_space(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "operationSpace": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.30, 52.50], [13.45, 52.50], [13.45, 52.58], [13.30, 52.58], [13.30, 52.50]]]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_different_attributes_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:City:Paris"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [2.2945, 48.8584]
            }
        },
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "en": "Champ de Mars",
                "fr": "Champ de Mars"
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_different_attributes_instances_and_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": [
            {
                "type": "Property",
                "value": "Eiffel Tower",
                "datasetId": "urn:ngsi-ld:Dataset:01"
            },
            {
                "type": "Property",
                "value": "La Tour Eiffel",
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "locatedAt": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:City:Paris",
                "datasetId": "urn:ngsi-ld:Dataset:01"
            },
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:District:7th",
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "location": [
            {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [2.2945, 48.8584]
                },
                "datasetId": "urn:ngsi-ld:Dataset:01"
            },
            {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [2.2944, 48.8583]
                },
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "street": [
            {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Champ de Mars",
                    "fr": "Champ de Mars"
                },
                "datasetId": "urn:ngsi-ld:Dataset:01"
            },
            {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Avenue Gustave Eiffel",
                    "fr": "Avenue Gustave Eiffel"
                },
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_different_default_attributes_instances_and_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": [
            {
                "type": "Property",
                "value": "Eiffel Tower"
            },
            {
                "type": "Property",
                "value": "La Tour Eiffel",
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "locatedAt": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:City:Paris"
            },
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:District:7th",
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "location": [
            {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [2.2945, 48.8584]
                }
            },
            {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [2.2944, 48.8583]
                },
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "street": [
            {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Champ de Mars",
                    "fr": "Champ de Mars"
                }
            },
            {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Avenue Gustave Eiffel",
                    "fr": "Avenue Gustave Eiffel"
                },
                "datasetId": "urn:ngsi-ld:Dataset:02"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_vocab_property_string(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "vocabProperty": {
            "type": "VocabProperty",
            "vocab": "residential"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_vocab_property_sub_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "vocabProperty": {
            "type": "VocabProperty",
            "vocab": "residential",
            "accuracy": {
                "type": "Property",
                "value": 0.95
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_language_property_sub_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "en": "Champ de Mars",
                "fr": "Champ de Mars"
            },
            "verified": {
                "type": "Property",
                "value": true
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_json_property_object_sub_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "jsonProperty": {
            "type": "JsonProperty",
            "json": {
                "id": "original-id",
                "data": [1, 2, 3]
            },
            "verified": {
                "type": "Property",
                "value": true
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_minimal(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_speed_two_datasetid(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 55,
                "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
            },
            {
                "type": "Property",
                "value": 54.5,
                "datasetId": "urn:ngsi-ld:Property:speedometer"
            }
        ],
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Person:Bob"
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_two_datasetid_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 55,
                "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
            },
            {
                "type": "Property",
                "value": 54.5,
                "datasetId": "urn:ngsi-ld:Property:speedometer"
            }
        ],
        "brandName": {
            "type": "Property",
            "value": "Mercedes"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_speed_only_multi_instances(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 55
            },
            {
                "type": "Property",
                "value": 54.5,
                "datasetId": "urn:ngsi-ld:Property:speedometer"
            },
            {
                "type": "Property",
                "value": 56,
                "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

fn vehicle_speed_multi_instances(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 55
            },
            {
                "type": "Property",
                "value": 54.5,
                "datasetId": "urn:ngsi-ld:Property:speedometer"
            }
        ],
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

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

async fn create_entity_json(client: &Client, entity: &Value, context_url: &str) -> StatusCode {
    // Strip @context from body - application/json must not contain @context per NGSI-LD 6.3.5
    let mut body = entity.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.remove("@context");
    }
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(context_url))
        .json(&body)
        .send()
        .await
        .expect("create_entity_json request failed");
    resp.status()
}

async fn get_entity(client: &Client, id: &str, query: &str, accept: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/entities/{encoded}{sep}{query}")).header("Accept", accept);
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn append_entity_attrs(client: &Client, id: &str, fragment: &Value, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .post(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("append_entity_attrs request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn append_entity_attrs_with_options(client: &Client, id: &str, fragment: &Value, options: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .post(format!("{BASE_URL}/entities/{encoded}/attrs?options={options}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("append_entity_attrs_with_options request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn append_entity_attrs_json(client: &Client, id: &str, fragment: &Value, context_url: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .post(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(context_url))
        .json(fragment)
        .send()
        .await
        .expect("append_entity_attrs_json request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn append_entity_attrs_raw(client: &Client, id: &str, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .post(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("append_entity_attrs_raw request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn update_entity_attrs(client: &Client, id: &str, fragment: &Value, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("update_entity_attrs request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn update_entity_attrs_json(client: &Client, id: &str, fragment: &Value, context_url: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(context_url))
        .json(fragment)
        .send()
        .await
        .expect("update_entity_attrs_json request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn update_entity_attrs_raw(client: &Client, id: &str, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("update_entity_attrs_raw request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn partial_update_attr(client: &Client, id: &str, attr_name: &str, fragment: &Value, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("partial_update_attr request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn partial_update_attr_with_datasetid(
    client: &Client,
    id: &str,
    attr_name: &str,
    dataset_id: &str,
    fragment: &Value,
    link: Option<&str>,
) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let encoded_dataset = urlencoding::encode(dataset_id);
    let mut req = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}?datasetId={encoded_dataset}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("partial_update_attr_with_datasetid request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn partial_update_attr_raw(client: &Client, id: &str, attr_name: &str, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .patch(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("partial_update_attr_raw request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn replace_entity_attr(client: &Client, id: &str, attr_name: &str, fragment: &Value, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .put(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("replace_entity_attr request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn replace_entity_attr_json(client: &Client, id: &str, attr_name: &str, fragment: &Value, context_url: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .put(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(context_url))
        .json(fragment)
        .send()
        .await
        .expect("replace_entity_attr_json request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn replace_entity_attr_with_datasetid(
    client: &Client,
    id: &str,
    attr_name: &str,
    dataset_id: &str,
    fragment: &Value,
    link: Option<&str>,
) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let encoded_dataset = urlencoding::encode(dataset_id);
    let mut req = client
        .put(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}?datasetId={encoded_dataset}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("replace_entity_attr_with_datasetid request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity_attr(client: &Client, id: &str, attr_name: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client.delete(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}"));
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("delete_entity_attr request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity_attr_with_datasetid(client: &Client, id: &str, attr_name: &str, dataset_id: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let encoded_dataset = urlencoding::encode(dataset_id);
    let mut req = client.delete(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}?datasetId={encoded_dataset}"));
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("delete_entity_attr_with_datasetid request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity_attr_delete_all(client: &Client, id: &str, attr_name: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client.delete(format!("{BASE_URL}/entities/{encoded}/attrs/{attr_name}?deleteAll=true"));
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("delete_entity_attr_delete_all request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Send `method` at a raw `path`, bypassing the typed per-operation helpers so
/// a test can drive a malformed or unroutable attribute URL.
async fn request_attrs_method(client: &Client, method: Method, path: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let builder = client
        .request(method, format!("{BASE_URL}{path}"))
        .header("Content-Type", "application/ld+json");
    let resp = if let Some(b) = body {
        builder.json(b).send().await.expect("request failed")
    } else {
        builder.send().await.expect("request failed")
    };
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}
