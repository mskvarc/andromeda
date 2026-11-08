//! Written documents in their expanded form: the Temporal Evolution
//! representation the temporal API assembles (several instances of one
//! dataset, `instanceId`, `deletedAt` tombstones), and the deletions a merge
//! writes (a whole attribute, a structured member, a language, a member's
//! value).

use serde_json::{Value, json};

/// The NGSI-LD core namespace.
const NGSI_LD: &str = "https://uri.etsi.org/ngsi-ld/";
/// The default-context namespace.
const DEFAULT_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
/// The `DateTime` datatype.
const DATE_TIME: &str = "https://uri.etsi.org/ngsi-ld/DateTime";

/// A core IRI.
fn core(term: &str) -> String {
    format!("{NGSI_LD}{term}")
}

/// A default-context IRI.
fn user(term: &str) -> String {
    format!("{DEFAULT_CONTEXT}{term}")
}

/// A `DateTime` value object array.
fn date_time(text: &str) -> Value {
    json!([{"@type": DATE_TIME, "@value": text}])
}

/// Every written document, by what it exercises.
pub(crate) fn written_documents() -> Vec<(&'static str, Value)> {
    vec![
        ("temporal-evolution", temporal_evolution()),
        ("merge-deletions", merge_deletions()),
        ("value-deletions", value_deletions()),
    ]
}

/// A Temporal Evolution of every family the temporal API renders.
fn temporal_evolution() -> Value {
    let mut speed_1 = serde_json::Map::new();
    speed_1.insert("@type".to_owned(), json!([core("Property")]));
    speed_1.insert(core("hasValue"), json!([{"@value": 120}]));
    speed_1.insert(core("observedAt"), date_time("2020-01-01T00:00:00Z"));
    speed_1.insert(core("instanceId"), json!([{"@id": "urn:ngsi-ld:attribute:instance:1"}]));
    speed_1.insert(core("datasetId"), json!([{"@id": "urn:ngsi-ld:Dataset:d"}]));
    speed_1.insert(core("createdAt"), date_time("2020-01-01T00:00:01.000000Z"));
    speed_1.insert(core("modifiedAt"), date_time("2020-01-01T00:00:01.000000Z"));
    let mut speed_2 = speed_1.clone();
    speed_2.insert(core("hasValue"), json!([{"@value": 80.25}]));
    speed_2.insert(core("observedAt"), date_time("2020-01-02T00:00:00Z"));
    speed_2.insert(core("instanceId"), json!([{"@id": "urn:ngsi-ld:attribute:instance:2"}]));
    json!([{
        "@id": "urn:ngsi-ld:Vehicle:temporal",
        "@type": [user("Vehicle")],
        (user("speed")): [
            Value::Object(speed_1),
            Value::Object(speed_2),
            {
                "@type": [core("Property")],
                (core("hasValue")): [{"@value": "urn:ngsi-ld:null"}],
                (core("deletedAt")): date_time("2020-01-03T00:00:00Z"),
                (core("instanceId")): [{"@id": "urn:ngsi-ld:attribute:instance:3"}]
            }
        ],
        (user("location")): [{
            "@type": [core("GeoProperty")],
            (core("hasValue")): [{"@type": ["https://purl.org/geojson/vocab#Point"], "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1}, {"@value": 2.5}]}]}],
            (core("observedAt")): date_time("2020-01-01T00:00:00Z")
        }],
        (user("name")): [{
            "@type": [core("LanguageProperty")],
            (core("hasLanguageMap")): [{"@value": "Bus", "@language": "en"}, {"@value": "Autobus", "@language": "fr"}],
            (core("observedAt")): date_time("2020-01-01T00:00:00Z")
        }],
        (user("isParked")): [
            {"@type": [core("Relationship")], (core("hasObject")): [{"@id": "urn:ngsi-ld:Parking:1"}], (core("observedAt")): date_time("2020-01-01T00:00:00Z")},
            {"@type": [core("Relationship")], (core("hasObject")): [{"@id": "urn:ngsi-ld:null"}], (core("deletedAt")): date_time("2020-01-04T00:00:00Z")}
        ],
        (user("route")): [{
            "@type": [core("ListRelationship")],
            (core("hasObjectList")): [{"@list": [{"@id": "urn:ngsi-ld:Stop:1"}, {(core("hasObject")): [{"@id": "urn:ngsi-ld:Stop:2"}]}]}]
        }],
        (user("config")): [{"@type": [core("JsonProperty")], (core("hasJSON")): [{"@type": "@json", "@value": {"a": [1, 2]}}]}],
        (user("category")): [{"@type": [core("VocabProperty")], (core("hasVocab")): [{"@id": user("commercial")}]}],
        (core("scope")): [{"@value": "/Fleet"}]
    }])
}

/// The instances of a reified Scope history (clause 4.5.6): one dated by
/// `createdAt` and `modifiedAt`, then one ending the Scope with the NGSI-LD
/// Null beside its `deletedAt`.
pub(crate) fn scope_history_instances() -> Vec<Value> {
    vec![
        json!({
            "@type": [core("Property")],
            (core("hasValue")): [{"@value": "/Madrid/Gardens"}],
            (core("observedAt")): date_time("2025-01-01T00:00:00Z"),
            (core("createdAt")): date_time("2025-01-01T00:00:01Z"),
            (core("modifiedAt")): date_time("2025-01-01T00:00:02Z")
        }),
        json!({
            "@type": [core("Property")],
            (core("hasValue")): [{"@value": "urn:ngsi-ld:null"}],
            (core("observedAt")): date_time("2025-01-02T00:00:00Z"),
            (core("deletedAt")): date_time("2025-01-02T00:00:00Z")
        }),
    ]
}

/// The deletions a merge fragment writes (clause 5.5.12).
fn merge_deletions() -> Value {
    json!([{
        (user("removed")): [{"@value": "urn:ngsi-ld:null"}],
        (user("address")): [{
            "@type": [core("Property")],
            (core("hasValue")): [{(user("street")): [{"@value": "urn:ngsi-ld:null"}], (user("number")): [{"@value": 7}]}],
            (core("unitCode")): [{"@value": "urn:ngsi-ld:null"}]
        }],
        (user("label")): [{
            "@type": [core("LanguageProperty")],
            (core("hasLanguageMap")): [{"@value": "urn:ngsi-ld:null", "@language": "fr"}, {"@value": "Label", "@language": "en"}]
        }],
        (user("readings")): [{"@type": [core("ListProperty")], (core("hasValueList")): [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}],
        (user("owner")): [{"@type": [core("Relationship")], (core("hasObject")): [{"@id": "urn:ngsi-ld:null"}], (core("observedAt")): date_time("urn:ngsi-ld:null")}]
    }])
}

/// The deletion of each family's whole value (clause 4.5.0), as a merge
/// writes it and a relayed notification reports it.
fn value_deletions() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Vehicle:deletions",
        "@type": [user("Vehicle")],
        (user("removed")): [{"@value": "urn:ngsi-ld:null"}],
        (user("speed")): [{"@type": [core("Property")], (core("hasValue")): [{"@value": "urn:ngsi-ld:null"}]}],
        (user("owner")): [{"@type": [core("Relationship")], (core("hasObject")): [{"@id": "urn:ngsi-ld:null"}]}],
        (user("readings")): [{"@type": [core("ListProperty")], (core("hasValueList")): [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}],
        (user("name")): [{"@type": [core("LanguageProperty")], (core("hasLanguageMap")): [{"@value": "urn:ngsi-ld:null"}]}],
        (user("rooms")): [{"@type": [core("ListRelationship")], (core("hasObjectList")): [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}],
        (user("category")): [{"@type": [core("VocabProperty")], (core("hasVocab")): [{"@id": "urn:ngsi-ld:null"}]}],
        (user("config")): [{"@type": [core("JsonProperty")], (core("hasJSON")): [{"@type": "@json", "@value": "urn:ngsi-ld:null"}]}]
    }])
}
