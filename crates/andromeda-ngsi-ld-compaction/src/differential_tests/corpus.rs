//! The stored documents the differential tests compact.
//!
//! Each is written as a client writes it, expanded by the processor and
//! stamped with the clause 4.8 instance times exactly as a create stores it, so
//! the corpus is what the store holds: every attribute family, sub-attributes
//! several levels deep, several datasets of one attribute, structured values
//! with node identifiers, typed and language-tagged literals, language maps,
//! vocabulary terms, JSON literals, lists of lists, both `ListRelationship`
//! spellings, every geometry type in two and three dimensions, the clause
//! 4.7.2 string geometry, scope, entity and instance expiry.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, system_instant::SystemInstant};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

/// One stored document.
pub(crate) struct StoredDocument {
    /// What the document exercises.
    pub(crate) name: &'static str,
    /// The stored expanded document.
    pub(crate) expanded: Value,
}

/// A compact entity and the context it is written with.
struct Written {
    /// What the document exercises.
    name: &'static str,
    /// The entity as a client writes it.
    entity: Value,
    /// The context it is written under.
    context: ContextSpec,
}

/// Expand and stamp every corpus entity as a create stores it.
pub(crate) async fn stored_documents(engine: &JsonLdEngine) -> Vec<StoredDocument> {
    let stamped_at = SystemInstant::new(Utc.with_ymd_and_hms(2025, 6, 1, 8, 30, 15).single().expect("a valid instant"));
    let mut stored = Vec::new();
    for written in written_entities() {
        let expanded = engine.expand(written.entity, &written.context).await.expect("the corpus entity expands");
        let mut state = EntityState::from_slice(&serde_json::to_vec(&expanded).expect("the expansion renders")).expect("the corpus entity is a state");
        state.stamp_attribute_instants(None, stamped_at);
        let expanded = serde_json::to_value(&state).expect("the stamped state renders");
        stored.push(StoredDocument { name: written.name, expanded });
    }
    stored
}

/// A compact entity written under the core context alone.
fn core(name: &'static str, entity: Value) -> Written {
    Written {
        name,
        entity,
        context: ContextSpec::none(),
    }
}

/// Every corpus entity.
fn written_entities() -> Vec<Written> {
    vec![
        core("every-family", every_family()),
        core("structured-values", structured_values()),
        core("datasets-and-expiry", datasets_and_expiry()),
        core("geometries", geometries()),
        core("lists", lists()),
        core("language-vocab-json", language_vocab_json()),
        core("relationships", relationships()),
        core("deep-sub-attributes", deep_sub_attributes()),
        core("unrestricted-value-names", unrestricted_value_names()),
        core("minimal", json!({"id": "urn:ngsi-ld:Thing:minimal", "type": "Thing"})),
        core(
            "http-identifiers",
            json!({
                "id": "http://example.org/entities/e1",
                "type": "Vehicle",
                "isParked": {"type": "Relationship", "object": "http://example.org/entities/parking"}
            }),
        ),
        Written {
            name: "context-shaped-values",
            entity: json!({
                "id": "urn:ngsi-ld:Thing:shaped",
                "type": "Thing",
                "address": {"type": "Property", "value": {"seq": [1, "two", {"x": 1}], "payload": {"raw": [1, null]}}}
            }),
            context: ContextSpec::from_body(json!({
                "seq": {"@id": "https://uri.etsi.org/ngsi-ld/default-context/seq", "@container": "@list"},
                "payload": {"@id": "https://uri.etsi.org/ngsi-ld/default-context/payload", "@type": "@json"}
            })),
        },
    ]
}

/// Every attribute family once, with sub-attributes, scope and expiry.
fn every_family() -> Value {
    let exact: Value = serde_json::from_str("0.100000000000000005").expect("an exact JSON number");
    json!({
        "id": "urn:ngsi-ld:Vehicle:every-family",
        "type": ["Vehicle", "http://example.org/vehicle/Car"],
        "scope": ["/Madrid/Gardens", "/Company/Fleet"],
        "expiresAt": "2099-01-01T00:00:00Z",
        "brandName": {
            "type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:03:00Z",
            "rating": {"type": "Property", "value": 4.5, "unitCode": "C62", "reviewedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"}}
        },
        "speed": [
            {"type": "Property", "value": 55, "datasetId": "urn:ngsi-ld:Dataset:gps", "unitCode": "KMH"},
            {"type": "Property", "value": exact, "datasetId": "urn:ngsi-ld:Dataset:obd"}
        ],
        "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:Downtown1", "objectType": "OffStreetParking", "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:Bob"}},
        "passengers": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Person:1"}, {"object": "urn:ngsi-ld:Person:2"}]},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
        "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt", "@none": "Main"}},
        "config": {"type": "JsonProperty", "json": {"nested": {"n": 0.5, "list": [1, "two", null, true]}}},
        "readings": {"type": "ListProperty", "valueList": [1, 2.5, "three"]},
        "category": {"type": "VocabProperty", "vocab": "commercial"},
        "tags": {"type": "Property", "value": ["a", "b"]},
        "untagged": {"type": "Property", "value": []},
        "flag": {"type": "Property", "value": false}
    })
}

/// Structured Property values: nested objects, node identifiers and
/// references, typed and language-tagged literals.
fn structured_values() -> Value {
    json!({
        "id": "urn:ngsi-ld:Building:structured",
        "type": "Building",
        "address": {"type": "Property", "value": {"street": "Main", "number": 5, "tags": ["a", "b"], "flats": [], "inner": {"deep": {"deeper": true}}}},
        "owner": {"type": "Property", "value": {"id": "urn:ngsi-ld:Person:owner", "name": "Owner"}},
        "pointer": {"type": "Property", "value": {"id": "urn:ngsi-ld:Thing:pointed"}},
        "shape": {"type": "Property", "value": {"type": "Point", "coordinates": [1, 2]}},
        "records": {"type": "Property", "value": [{"a": 1}, {"a": 2, "b": "x"}]},
        "testedAt": {"type": "Property", "value": {"@value": "2020-01-01T00:00:00Z", "@type": "DateTime"}},
        "greeting": {"type": "Property", "value": {"@value": "hola", "@language": "es"}},
        "arabic": {"type": "Property", "value": {"@value": "مرحبا", "@language": "ar", "@direction": "rtl"}},
        "count": {"type": "Property", "value": {"@value": "7", "@type": "http://www.w3.org/2001/XMLSchema#integer"}, "valueType": "Integer"},
        "nothing": {"type": "Property", "value": {"nested": null, "kept": 1}}
    })
}

/// Structured values and foreign members whose keys, `@id` and `@type` are no
/// NGSI-LD names (clauses 3.1 and 4.6.3 admit any JSON object as a value):
/// expansion keeps each as text that is no well-formed IRI.
fn unrestricted_value_names() -> Value {
    json!({
        "id": "urn:ngsi-ld:Shop:unrestricted",
        "type": "Shop",
        "hours": {"type": "Property", "value": {"opening hours": "9-5", "nested": {"a b": [{"c d": 1}]}}},
        "address": {"type": "Property", "value": {"id": "abc", "type": "Postal Address", "street": "x"}},
        "parcel": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2], "plot name": "7"}}
    })
}

/// One attribute held as several datasets, with instance expiry, observation
/// times and instance identifiers.
fn datasets_and_expiry() -> Value {
    json!({
        "id": "urn:ngsi-ld:Sensor:datasets",
        "type": "Sensor",
        "temperature": [
            {"type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z", "accuracy": {"type": "Property", "value": 0.5}},
            {"type": "Property", "value": 21, "datasetId": "urn:ngsi-ld:Dataset:a", "expiresAt": "2020-01-01T00:00:00Z"},
            {"type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:b", "expiresAt": "2099-01-01T00:00:00Z", "instanceId": "urn:ngsi-ld:attribute:instance:b"}
        ],
        "humidity": {"type": "Property", "value": 40, "expiresAt": "2020-06-01T00:00:00Z"},
        "pressure": {"type": "Property", "value": 1013, "unitCode": "A97", "observedAt": "2025-01-01T00:00:00.123Z"}
    })
}

/// Every geometry type, in two and three dimensions, as the three entity
/// geo-properties, an ordinary attribute, a sub-attribute and a string, and
/// geometries carrying a `bbox` and foreign members (IETF RFC 7946 sections 5
/// and 6.1) as a `GeoProperty` and as a Property value.
fn geometries() -> Value {
    json!({
        "id": "urn:ngsi-ld:Area:geometries",
        "type": "Area",
        "location": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547, 34.5]}},
            {"type": "GeoProperty", "datasetId": "urn:ngsi-ld:Dataset:gps", "value": {"type": "Point", "coordinates": [13.4, 52.55]}}
        ],
        "observationSpace": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]], [[2, 2], [3, 2], [3, 3], [2, 2]]]}},
        "operationSpace": {"type": "GeoProperty", "value": {"type": "MultiPolygon", "coordinates": [[[[0, 0], [1, 0], [1, 1], [0, 0]]], [[[5, 5, 1], [6, 5, 1], [6, 6, 1], [5, 5, 1]]]]}},
        "route": {"type": "GeoProperty", "value": {"type": "MultiLineString", "coordinates": [[[0, 0], [1, 1]], [[2, 2], [3, 3], [4, 4]]]}, "unitCode": "MTR"},
        "stops": {"type": "GeoProperty", "value": {"type": "MultiPoint", "coordinates": [[0.25, 0.5], [1.75, -2.125]]}, "valueType": "geojson:MultiPoint"},
        "path": {"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[-8.5, 41.2], [-8.6, 41.3]]},
            "origin": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}}},
        "written": {"type": "GeoProperty", "value": "{\"type\": \"Point\", \"coordinates\": [1, 2]}"},
        "parcel": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [4, 0], [4, 4], [0, 0]]], "bbox": [0, 0, 4, 4], "plotName": "plot 7", "survey": {"year": 2024, "marks": [1, 2]}}},
        "marker": {"type": "Property", "value": {"type": "Point", "coordinates": [1, 1, 3], "bbox": [1, 1, 3, 1, 1, 3], "plotName": "corner"}}
    })
}

/// Lists of values, of lists and of objects, and both `ListRelationship`
/// spellings.
fn lists() -> Value {
    json!({
        "id": "urn:ngsi-ld:Route:lists",
        "type": "Route",
        "matrix": {"type": "ListProperty", "valueList": [[1, 2], [3], []]},
        "records": {"type": "ListProperty", "valueList": [{"a": 1}, "text", 2]},
        "single": {"type": "ListProperty", "valueList": ["only"]},
        "empty": {"type": "ListProperty", "valueList": []},
        "stops": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Stop:1"}, {"id": "urn:ngsi-ld:Stop:2"}], "objectType": "Stop"}
    })
}

/// Language maps, vocabulary terms and JSON literals in their several shapes.
fn language_vocab_json() -> Value {
    json!({
        "id": "urn:ngsi-ld:Place:language",
        "type": "Place",
        "name": {"type": "LanguageProperty", "languageMap": {"en": ["One", "Uno"], "fr": "Un", "@none": "1"}},
        "label": {"type": "LanguageProperty", "languageMap": {"de": "Hallo"}},
        "category": {"type": "VocabProperty", "vocab": ["commercial", "https://schema.org/Place"]},
        "kind": {"type": "VocabProperty", "vocab": "Building"},
        "payload": {"type": "JsonProperty", "json": [{"a": 1}, [1, 2], "text"]},
        "scalarJson": {"type": "JsonProperty", "json": 12.50}
    })
}

/// Relationships with several targets and object types.
fn relationships() -> Value {
    json!({
        "id": "urn:ngsi-ld:Fleet:relationships",
        "type": "Fleet",
        "vehicles": {"type": "Relationship", "object": ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"], "objectType": ["Vehicle", "Car"]},
        "depot": [
            {"type": "Relationship", "object": "urn:ngsi-ld:Depot:1", "datasetId": "urn:ngsi-ld:Dataset:main"},
            {"type": "Relationship", "object": "urn:ngsi-ld:Depot:2", "observedAt": "2025-02-02T02:02:02Z"}
        ]
    })
}

/// Sub-attributes of sub-attributes.
fn deep_sub_attributes() -> Value {
    json!({
        "id": "urn:ngsi-ld:Device:deep",
        "type": "Device",
        "reading": {
            "type": "Property", "value": 1,
            "quality": {
                "type": "Property", "value": "good",
                "assessedBy": {
                    "type": "Relationship", "object": "urn:ngsi-ld:Person:assessor",
                    "certified": {"type": "Property", "value": true, "observedAt": "2024-04-04T04:04:04Z"}
                }
            },
            "position": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 1]}}
        }
    })
}
