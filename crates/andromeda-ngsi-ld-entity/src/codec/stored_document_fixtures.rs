//! Stored documents the codec tests and the conversion tests share: every
//! family with every optional member, instances in stored order, and the
//! spellings of an object list.

use serde_json::{Value, json};

pub(crate) const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
pub(crate) const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";
pub(crate) const RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/Relationship";
pub(crate) const LANGUAGE_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/LanguageProperty";
pub(crate) const VOCAB_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/VocabProperty";
pub(crate) const JSON_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/JsonProperty";
pub(crate) const LIST_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/ListProperty";
pub(crate) const LIST_RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/ListRelationship";
pub(crate) const DATE_TIME: &str = "https://uri.etsi.org/ngsi-ld/DateTime";
pub(crate) const NGSI: &str = "https://uri.etsi.org/ngsi-ld/";
pub(crate) const GEOJSON: &str = "https://purl.org/geojson/vocab#";
pub(crate) const COORDINATES: &str = "https://purl.org/geojson/vocab#coordinates";

/// The expanded IRI of an NGSI-LD core term.
pub(crate) fn ngsi(term: &str) -> String {
    format!("{NGSI}{term}")
}

/// A typed `DateTime` member.
pub(crate) fn date_time(text: &str) -> Value {
    json!([{"@type": DATE_TIME, "@value": text}])
}

/// The system times every stored instance carries.
pub(crate) fn stamped(mut instance: Value) -> Value {
    if let Some(members) = instance.as_object_mut() {
        members.insert(ngsi("createdAt"), date_time("2026-10-01T17:39:35.497983Z"));
        members.insert(ngsi("modifiedAt"), date_time("2026-10-01T17:40:00Z"));
    }
    instance
}

/// An entity node with the given attribute members.
pub(crate) fn entity(members: &Value) -> Value {
    let mut node = json!({"@id": "urn:ngsi-ld:Building:1", "@type": ["https://ex.org/Building"]});
    if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
        node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    json!([node])
}

/// A Property instance with `value` and its system times.
pub(crate) fn property(value: &Value) -> Value {
    stamped(json!({"@type": [PROPERTY], ngsi("hasValue"): value}))
}

/// The sub-attributes of every family, nested twice.
pub(crate) fn nested_sub_attributes() -> Value {
    json!({
        "https://ex.org/accuracy": [{
            "@type": [PROPERTY],
            ngsi("hasValue"): [{"@value": 0.5}],
            "https://ex.org/method": [{
                "@type": [PROPERTY],
                ngsi("hasValue"): [{"@value": "gps"}],
                ngsi("observedAt"): date_time("2026-01-01T00:00:00Z")
            }]
        }]
    })
}

/// An entity holding every family with every optional member, sub-attributes
/// nested twice, multiple instances, a scope and an entity expiry.
pub(crate) fn every_family_document() -> Value {
    let mut speed = json!({
        "@type": [PROPERTY],
        ngsi("hasValue"): [{"@value": 55.5}],
        ngsi("unitCode"): [{"@value": "KMH"}],
        ngsi("hasValueType"): [{"@id": "https://ex.org/Speed"}],
        ngsi("observedAt"): date_time("2026-01-01T00:00:00.5Z"),
        ngsi("expiresAt"): date_time("2027-01-01T00:00:00Z"),
        ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]
    });
    if let (Some(speed), Some(nested)) = (speed.as_object_mut(), nested_sub_attributes().as_object()) {
        speed.extend(nested.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    entity(&json!({
        ngsi("scope"): [{"@value": "/Madrid/Gardens"}, {"@value": "/Madrid/Parks"}],
        ngsi("expiresAt"): date_time("2030-01-01T00:00:00Z"),
        "https://ex.org/speed": [stamped(speed), property(&json!([{"@value": 40}])), stamped(json!({
            "@type": [PROPERTY],
            ngsi("hasValue"): [{"@value": 41}],
            ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:b"}]
        }))],
        ngsi("location"): [stamped(json!({
            "@type": [GEO_PROPERTY],
            ngsi("hasValue"): [{"@type": [format!("{GEOJSON}Point")], format!("{GEOJSON}coordinates"): [{"@list": [{"@value": 13.38}, {"@value": 52.52}, {"@value": 34.5}]}]}],
            ngsi("unitCode"): [{"@value": "MTR"}],
            ngsi("hasValueType"): [{"@id": "https://ex.org/Location"}]
        }))],
        "https://ex.org/owner": [stamped(json!({
            "@type": [RELATIONSHIP],
            ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Person:1"}, {"@id": "urn:ngsi-ld:Person:2"}],
            ngsi("hasObjectType"): [{"@id": "https://ex.org/Person"}]
        }))],
        "https://ex.org/name": [stamped(json!({
            "@type": [LANGUAGE_PROPERTY],
            ngsi("hasLanguageMap"): [
                {"@value": "Hauptbahnhof", "@language": "de"},
                {"@value": "Central Station", "@language": "en"},
                {"@value": "Main Station", "@language": "en"},
                {"@value": "Station"}
            ],
            ngsi("hasValueType"): [{"@id": "https://ex.org/Name"}]
        }))],
        "https://ex.org/category": [stamped(json!({
            "@type": [VOCAB_PROPERTY],
            ngsi("hasVocab"): [{"@id": "https://ex.org/commercial"}, {"@id": "https://ex.org/office"}]
        }))],
        "https://ex.org/config": [stamped(json!({
            "@type": [JSON_PROPERTY],
            ngsi("hasJSON"): [{"@type": "@json", "@value": {"a": [1, 2.5, {"b": null}], "c": "text"}}]
        }))],
        "https://ex.org/readings": [stamped(json!({
            "@type": [LIST_PROPERTY],
            ngsi("hasValueList"): [{"@list": [{"@value": 1}, {"@value": "two"}, {"@list": [{"@value": true}]}]}],
            ngsi("unitCode"): [{"@value": "CEL"}]
        }))],
        "https://ex.org/route": [stamped(json!({
            "@type": [LIST_RELATIONSHIP],
            ngsi("hasObjectList"): [{"@list": [
                {"@id": "urn:ngsi-ld:Stop:1"},
                {ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:2"}]}
            ]}],
            ngsi("hasObjectType"): [{"@id": "https://ex.org/Stop"}]
        }))],
        "https://ex.org/address": [property(&json!([{
            "@id": "urn:ngsi-ld:Address:1",
            "@type": ["https://ex.org/PostalAddress"],
            "https://ex.org/street": [{"@value": "Main", "@language": "en"}],
            "https://ex.org/since": [{"@type": DATE_TIME, "@value": "2020-01-01T00:00:00Z"}],
            "https://ex.org/owner": [{"@id": "urn:ngsi-ld:Person:3"}],
            "https://ex.org/floors": [{"@list": [{"@value": 1}, {"@value": 2}]}],
            "https://ex.org/inner": [{"https://ex.org/deep": [{"@value": false}]}],
            "https://ex.org/raw": [{"@type": "@json", "@value": [1, 2]}]
        }, {"@value": "second value"}]))]
    }))
}

/// An attribute whose default instance stands between two named ones.
pub(crate) fn stored_order_document() -> Value {
    entity(&json!({
        "https://ex.org/speed": [
            stamped(json!({"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 1}], ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:z"}]})),
            property(&json!([{"@value": 2}])),
            stamped(json!({"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 3}], ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]}))
        ]
    }))
}

/// A `ListRelationship` whose items are written wrapped, bare and as plain
/// strings.
pub(crate) fn list_relationship_spellings_document() -> Value {
    entity(&json!({
        "https://ex.org/route": [stamped(json!({
            "@type": [LIST_RELATIONSHIP],
            ngsi("hasObjectList"): [{"@list": [
                {ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:2"}]},
                {"@id": "urn:ngsi-ld:Stop:1"},
                {"@value": "urn:ngsi-ld:Stop:3"}
            ]}]
        }))]
    }))
}
