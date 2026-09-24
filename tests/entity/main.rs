//! Entity consumption integration tests for the NGSI-LD context broker.
//!
//! Tests entity query and retrieve operations:
//! - Query: by id, type, id pattern, attrs, geoquery, scope, pagination, count, q filter
//! - Retrieve: by id, attrs filter, representations, `GeoJSON`, linked entities
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `entity_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod create;
mod delete;
mod delete_by_query;
mod merge;
mod query;
mod replace;
mod retrieve;

use andromeda_test_client::header::jsonld_link_header as try_jsonld_link_header;
use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, environment::TestEnvironment};
use reqwest::{Client, Method, StatusCode, header::HeaderValue};
use serde_json::{Value, json};

fn jsonld_link_header(context_url: &str) -> HeaderValue {
    try_jsonld_link_header(context_url).expect("valid JSON-LD Link header")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
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
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_simple_attributes_second(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 6,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        },
        "locatedIn": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:City:Pisa"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_simple_attributes_third(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Big Ben"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 8,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {
            "type": "Property",
            "value": true
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_location(id: &str) -> Value {
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
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.3986, 52.5547]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_location_second(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Brandenburg Gate"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.388_930, 52.534_473]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_location_third(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Victory Column"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.35012, 52.51434]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_two_geometry_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.3986, 52.5547]
            }
        },
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

fn building_relationship(id: &str, city_id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": city_id
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_language_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "LanguageProperty",
            "languageMap": {
                "en": "Eiffel Tower",
                "fr": "Tour Eiffel",
                "de": "Eiffelturm"
            }
        },
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

fn building_location_polygon(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.286_590_6,52.564_864_5],[13.287_963_9,52.564_864_5],[13.279_724_1,52.498_867_9],[13.477_478,52.471_270_3],[13.504_943_8,52.537_308_4],[13.286_590_6,52.564_864_5]]]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_location_polygon_second(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.3,52.48],[13.3,52.52],[13.38,52.52],[13.38,52.48],[13.3,52.48]]]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_with_different_type(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Parking",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_with_two_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": ["Building", "TouristDestination"],
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_with_one_scope(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_with_many_scopes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "scope": ["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"],
        "@context": [COMPOUND_CONTEXT]
    })
}

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
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn parking_simple_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "OffStreetParking",
        "name": {
            "type": "Property",
            "value": "Downtown Parking"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.4050, 52.5200]
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn city_minimal(id: &str) -> Value {
    json!({
        "id": id,
        "type": "City",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn city_relationship(id: &str, country_id: &str) -> Value {
    json!({
        "id": id,
        "type": "City",
        "locatedAt": {
            "type": "Relationship",
            "object": country_id
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn country_minimal(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Country",
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_relationship_of_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z",
            "hasWeatherObserved": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Sensor:43a1af27-4aa7-44fe-bee9-fe76d56978a4"
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_non_core_geoproperty(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "customLocation": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.3986, 52.5547]
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

fn building_json_property_object(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "jsonProperty": {
            "type": "JsonProperty",
            "json": {
                "key1": "value1",
                "key2": 42,
                "nested": {
                    "inner": true
                }
            }
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_json_property_array(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "jsonProperty": {
            "type": "JsonProperty",
            "json": [
                {"item": 1},
                {"item": 2}
            ]
        },
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

fn building_vocab_property_array(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "vocabProperty": {
            "type": "VocabProperty",
            "vocab": ["residential", "commercial"]
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_merge_data(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2024-01-01T00:00:00.000Z"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_simple_attributes_second_for_merge(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "almostFull": {
            "type": "Property",
            "value": true
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 6
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_relationship_for_merge(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:City:Paris"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_only_airqualitylevel_value(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "airQualityLevel": {
            "type": "Property",
            "value": 1
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

fn building_locatedat_and_name(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:City:Pisa"
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

async fn query_entities(client: &Client, scope: &TestEnvironment, query: &str, accept: &str, link: Option<&str>) -> (StatusCode, Value, Option<String>) {
    let query = scope.scope_query(query);
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.get(format!("{BASE_URL}/entities{sep}{query}")).header("Accept", accept);
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("query_entities request failed");
    let status = resp.status();
    let count = resp.headers().get("NGSILD-Results-Count").map(|v| v.to_str().unwrap().to_owned());
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body, count)
}

async fn batch_query(client: &Client, body: &Value, accept: &str, link: Option<&str>) -> (StatusCode, Value) {
    let mut req = client
        .post(format!("{BASE_URL}/entityOperations/query"))
        .header("Content-Type", "application/ld+json")
        .header("Accept", accept);
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(body).send().await.expect("batch_query request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Create entity and return (status, `location_header`, body)
async fn create_entity_full(client: &Client, entity: &Value, content_type: &str, link: Option<&str>) -> (StatusCode, Option<String>, Value) {
    let mut req = client.post(format!("{BASE_URL}/entities")).header("Content-Type", content_type);
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(entity).send().await.expect("create_entity_full request failed");
    let status = resp.status();
    let location = resp.headers().get("Location").and_then(|v| v.to_str().ok()).map(ToOwned::to_owned);
    let body: Value = resp.json().await.unwrap_or_default();
    (status, location, body)
}

async fn create_entity_raw(client: &Client, raw: &[u8], content_type: &str) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", content_type)
        .body(raw.to_vec())
        .send()
        .await
        .expect("create_entity_raw request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity(client: &Client, id: &str) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let resp = client
        .delete(format!("{BASE_URL}/entities/{encoded}"))
        .send()
        .await
        .expect("delete_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn merge_entity(client: &Client, id: &str, fragment: &Value, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client
        .patch(format!("{BASE_URL}/entities/{encoded}"))
        .header("Content-Type", "application/ld+json");
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.json(fragment).send().await.expect("merge_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn replace_entity(client: &Client, id: &str, replacement: &Value, content_type: &str, link: Option<&str>) -> (StatusCode, Value) {
    let encoded = urlencoding::encode(id);
    let mut req = client.put(format!("{BASE_URL}/entities/{encoded}")).header("Content-Type", content_type);
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    // Strip @context from body when using application/json (must not contain @context per NGSI-LD 6.3.5)
    let body = if content_type == "application/json" {
        let mut b = replacement.clone();
        if let Some(obj) = b.as_object_mut() {
            obj.remove("@context");
        }
        b
    } else {
        replacement.clone()
    };
    let resp = req.json(&body).send().await.expect("replace_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entities_by_query(client: &Client, scope: &TestEnvironment, query: &str, link: Option<&str>) -> (StatusCode, Value) {
    let query = scope.scope_query(query);
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client.delete(format!("{BASE_URL}/entities{sep}{query}"));
    if let Some(ctx) = link {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("delete_entities_by_query request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// Send `method` at `/entities` — the collection URL, which accepts no
/// entity-modifying method — to assert the broker rejects it.
async fn request_method_on_collection(client: &Client, method: Method, body: Option<&Value>) -> (StatusCode, Value) {
    let builder = client
        .request(method, format!("{BASE_URL}/entities"))
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
