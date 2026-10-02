//! The reading of attribute members from a compact node, held to the decoding
//! it replaced.
//!
//! The oracle is the member decoding as it stood before the reading was
//! shared with compaction: an object member decoded as one instance -- a
//! `GeoProperty` by its geometry-aware reader, every other family by the
//! derived decoding -- an array as instances, and anything that fails carried
//! verbatim. Every member of the corpus must read to the same variant holding
//! the same content, which covers every way an instance falls back to the
//! verbatim member.

use crate::{
    attribute::{
        geo_property::GeoProperty,
        instance::{Attribute, AttributeValue},
    },
    entity::Entity,
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use pretty_assertions::assert_eq;
use serde_json::{Number, Value, json};

/// One instance as the replaced decoding read it.
fn oracle_instance(instance: &Value) -> Result<Attribute, serde_json::Error> {
    match instance.as_object() {
        Some(members) if members.get("type").and_then(Value::as_str) == Some(AttributeType::GeoProperty.short_name()) => {
            GeoProperty::from_instance(instance).map(Attribute::GeoProperty)
        }
        Some(_) | None => deserialize_exact(instance),
    }
}

/// One member as the replaced decoding read it.
fn oracle(member: Value) -> AttributeValue {
    match member {
        Value::Object(_) => match oracle_instance(&member) {
            Ok(instance) => AttributeValue::Single(Box::new(instance)),
            Err(_) => AttributeValue::Raw(member),
        },
        Value::Array(items) => match items.iter().map(oracle_instance).collect::<Result<Vec<Attribute>, serde_json::Error>>() {
            Ok(instances) => AttributeValue::Multiple(instances),
            Err(_) => AttributeValue::Raw(Value::Array(items)),
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => AttributeValue::Raw(member),
    }
}

/// The variant a member read as, and its content rendered back to JSON.
fn reading(value: &AttributeValue) -> (&'static str, String) {
    let variant = match value {
        AttributeValue::Single(_) => "single",
        AttributeValue::Multiple(_) => "multiple",
        AttributeValue::Raw(_) => "raw",
    };
    (variant, serde_json::to_string(value).expect("an attribute member renders"))
}

/// Read `member` both ways, assert they agree, and answer the variant.
fn read_alike(member: &Value) -> &'static str {
    // clone: each reading consumes or borrows its own copy of the member
    let expected = reading(&oracle(member.clone()));
    let from_member = reading(&AttributeValue::from_member(member.clone()));
    let from_node = reading(&AttributeValue::from_node(member).expect("a JSON tree is readable"));
    assert_eq!(from_member, expected, "{member}");
    // A member carried verbatim keeps its own spelling in `from_member` and the
    // reader's in `from_node`; the corpus is spelled as the reader spells it.
    assert_eq!(from_node, expected, "{member}");
    expected.0
}

/// Every family's instance with every member it may carry.
fn full_instances() -> Vec<Value> {
    vec![
        json!({"type": "Property", "value": 1.5, "unitCode": "CEL", "valueType": "Number", "previousValue": 2, "datasetId": "urn:ngsi-ld:Dataset:a",
            "observedAt": "2024-01-01T00:00:00Z", "expiresAt": "2030-01-01T00:00:00.120Z", "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z", "deletedAt": "2024-01-03T00:00:00Z", "instanceId": "urn:ngsi-ld:attribute:instance:1",
            "accuracy": {"type": "Property", "value": 0.5, "precision": {"type": "Property", "value": 1}}}),
        json!({"type": "Relationship", "object": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"], "objectType": ["A", "ex:B"], "previousObject": "urn:ngsi-ld:A:0",
            "entity": {"id": "urn:ngsi-ld:A:1", "type": "A", "name": {"type": "Property", "value": "a"}}}),
        json!({"type": "VocabProperty", "vocab": ["commercial", "ex:Offer"], "valueType": "xsd:string", "previousVocab": "private"}),
        json!({"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0, 12.5], [0.0, 0.0]]]},
            "previousValue": {"type": "Point", "coordinates": [1.0, 2.0]}, "valueType": "geojson:Polygon", "accuracy": {"type": "Property", "value": 3}}),
        json!({"type": "LanguageProperty", "languageMap": {"en": "a", "@none": "b", "de": ["c", "d"]}, "previousLanguageMap": {"en": "x"}, "valueType": "langString"}),
        json!({"type": "JsonProperty", "json": {"id": "x", "n": [1, 2.25]}, "previousJson": [1], "valueType": "json"}),
        json!({"type": "ListProperty", "valueList": [1, "a", {"type": "Point", "coordinates": [1.0, 2.0]}, [3]], "previousValueList": [0], "valueType": "Integer"}),
        json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:A:1"}, "urn:ngsi-ld:A:2"], "objectType": "A",
            "previousObjectList": ["urn:ngsi-ld:null"], "entityList": [{"id": "urn:ngsi-ld:A:1", "type": "A"}]}),
    ]
}

#[test]
fn every_family_with_every_member_reads_as_the_replaced_decoding_did() {
    for instance in full_instances() {
        assert_eq!(read_alike(&instance), "single");
    }
    assert_eq!(read_alike(&Value::Array(full_instances())), "multiple");
}

/// Every way an instance is refused carries the member verbatim.
#[test]
fn every_refused_instance_is_carried_verbatim_as_the_replaced_decoding_did() {
    let refused = [
        // No family, or no family by that name.
        json!({"value": 1}),
        json!({"type": 7, "value": 1}),
        json!({"type": "Foo", "value": 1}),
        json!({"type": "https://uri.etsi.org/ngsi-ld/Property", "value": 1}),
        // A family's own member missing.
        json!({"type": "Property"}),
        json!({"type": "Relationship"}),
        json!({"type": "VocabProperty"}),
        json!({"type": "GeoProperty"}),
        json!({"type": "LanguageProperty"}),
        json!({"type": "JsonProperty"}),
        json!({"type": "ListProperty"}),
        json!({"type": "ListRelationship"}),
        // A family's own member that is not what the family holds.
        json!({"type": "Property", "value": {"type": "Point", "coordinates": [200.0, 0.0]}}),
        json!({"type": "Property", "value": 1, "unitCode": "XYZ"}),
        json!({"type": "Property", "value": 1, "unitCode": 5}),
        json!({"type": "Property", "value": 1, "valueType": "bad ref"}),
        json!({"type": "Property", "value": 1, "previousValue": {"type": "Point", "coordinates": [1.0]}}),
        json!({"type": "Relationship", "object": "not a uri"}),
        json!({"type": "Relationship", "object": null}),
        json!({"type": "Relationship", "object": 5}),
        json!({"type": "Relationship", "object": ["urn:ngsi-ld:A:1", 5]}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": "bad ref"}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "previousObject": "not a uri"}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "entity": {"type": "A"}}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "entity": "urn:ngsi-ld:A:1"}),
        json!({"type": "VocabProperty", "vocab": "bad ref"}),
        json!({"type": "VocabProperty", "vocab": null}),
        json!({"type": "VocabProperty", "vocab": "a", "previousVocab": 1}),
        json!({"type": "GeoProperty", "value": json!({"type": "Point", "coordinates": [1.0, 2.0]}).to_string()}),
        json!({"type": "GeoProperty", "value": {"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}}),
        json!({"type": "GeoProperty", "value": {"type": "Point"}}),
        json!({"type": "GeoProperty", "value": {"type": "GeometryCollection", "geometries": []}}),
        json!({"type": "GeoProperty", "value": null}),
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "previousValue": "x"}),
        json!({"type": "LanguageProperty", "languageMap": {"en us": "a"}}),
        json!({"type": "LanguageProperty", "languageMap": {"en": 1}}),
        json!({"type": "LanguageProperty", "languageMap": "urn:ngsi-ld:null"}),
        json!({"type": "LanguageProperty", "languageMap": null}),
        json!({"type": "ListProperty", "valueList": 1}),
        json!({"type": "ListProperty", "valueList": null}),
        json!({"type": "ListProperty", "valueList": [{"type": "Point", "coordinates": [1.0, 99.0]}]}),
        json!({"type": "ListProperty", "valueList": [1], "previousValueList": 2}),
        json!({"type": "ListRelationship", "objectList": "urn:ngsi-ld:A:1"}),
        json!({"type": "ListRelationship", "objectList": [{"subject": "urn:ngsi-ld:A:1"}]}),
        json!({"type": "ListRelationship", "objectList": ["not a uri"]}),
        json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1"], "entityList": [{"id": "urn:ngsi-ld:A:1"}]}),
        // A shared member that is not what every family holds in it.
        json!({"type": "Property", "value": 1, "observedAt": "yesterday"}),
        json!({"type": "Property", "value": 1, "observedAt": 5}),
        json!({"type": "Property", "value": 1, "expiresAt": "2030-01-01"}),
        json!({"type": "Property", "value": 1, "createdAt": true}),
        json!({"type": "Property", "value": 1, "datasetId": "not a uri"}),
        json!({"type": "Property", "value": 1, "instanceId": 3}),
        // A sub-attribute named by no attribute name.
        json!({"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 1}}),
        json!({"type": "Property", "value": 1, "": 1}),
    ];
    for member in &refused {
        assert_eq!(read_alike(member), "raw");
    }
    // An array holding one refused instance, or anything but instances, is
    // carried whole.
    for member in [
        json!([{"type": "Property", "value": 1}, {"type": "Property"}]),
        json!([{"type": "Property", "value": 1}, 1]),
        json!([1, 2, 3]),
        json!([[{"type": "Property", "value": 1}]]),
    ] {
        assert_eq!(read_alike(&member), "raw");
    }
    for member in [json!(42), json!(1.25), json!("text"), json!(true), Value::Null] {
        assert_eq!(read_alike(&member), "raw");
    }
}

/// `null` is no value for every member a family may leave out, and a member
/// one family owns is a sub-attribute of any other.
#[test]
fn optional_nulls_and_foreign_members_read_as_the_replaced_decoding_did() {
    let accepted = [
        json!({"type": "Property", "value": null}),
        json!({"type": "Property", "value": 1, "unitCode": null, "valueType": null, "previousValue": null, "datasetId": null, "observedAt": null,
            "expiresAt": null, "createdAt": null, "modifiedAt": null, "deletedAt": null, "instanceId": null}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": null, "previousObject": null, "entity": null}),
        json!({"type": "JsonProperty", "json": null, "previousJson": null}),
        json!({"type": "ListProperty", "valueList": [], "previousValueList": null}),
        json!({"type": "ListRelationship", "objectList": [], "previousObjectList": null, "entityList": null}),
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0], "title": "a foreign member"}, "previousValue": null}),
        json!({"type": "LanguageProperty", "languageMap": {}, "previousLanguageMap": null}),
        // `unitCode` beside a list, `valueType` beside a Relationship, `entity`
        // beside a Property, `objectType` beside a Property: sub-attributes.
        json!({"type": "ListProperty", "valueList": [1], "unitCode": "CEL"}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "valueType": "Integer"}),
        json!({"type": "Property", "value": 1, "entity": {"id": "urn:ngsi-ld:A:1", "type": "A"}}),
        json!({"type": "Property", "value": 1, "objectType": "A", "object": "urn:ngsi-ld:A:1"}),
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "unitCode": "MTR"}),
        // A sub-attribute that is itself refused is carried verbatim inside a
        // typed instance.
        json!({"type": "Property", "value": 1, "note": {"type": "Property"}, "concise": "x", "deep": {"type": "Property", "value": 1, "deeper": {"type": "Relationship", "object": "x y"}}}),
    ];
    for member in &accepted {
        assert_eq!(read_alike(member), "single");
    }
    assert_eq!(read_alike(&json!([])), "multiple");
}

/// A number the reader spells otherwise is held in the reader's spelling, as
/// the replaced decoding held it.
#[test]
fn a_number_is_held_as_the_reader_spells_it() {
    let respelled = Value::Number(Number::from_string_unchecked("1E+2".to_owned()));
    let member = json!({"type": "Property", "value": [respelled], "sub": {"type": "Property", "value": 1}});
    let expected = reading(&oracle(member.clone())); // clone: the oracle consumes its copy
    assert_eq!(reading(&AttributeValue::from_member(member)), expected);
}

/// An entity reads from a node as it decodes from a document.
#[test]
fn an_entity_reads_from_a_node_as_it_decodes() {
    let documents = [
        json!({"id": "urn:ngsi-ld:T:1", "type": ["T", "U"], "scope": ["/a", "/b"], "expiresAt": "2030-01-01T00:00:00Z",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}, "observationSpace": null,
            "operationSpace": "concise", "createdAt": "2024-01-01T00:00:00Z", "modifiedAt": "2024-01-01T00:00:00Z", "deletedAt": null,
            "name": {"type": "Property", "value": "a"}, "raw": [1, 2]}),
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "scope": "/a"}),
    ];
    for document in &documents {
        let decoded: Entity = deserialize_exact(document).expect("the document decodes");
        let read = Entity::from_node(document).expect("the node reads");
        assert_eq!(
            serde_json::to_string(&read).expect("renders"),
            serde_json::to_string(&decoded).expect("renders")
        );
    }
    for refused in [
        json!({"type": "T"}),
        json!({"id": "urn:ngsi-ld:T:1"}),
        json!({"id": "not a uri", "type": "T"}),
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "bad name": 1}),
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "scope": "no scope"}),
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "expiresAt": "soon"}),
        json!([{"id": "urn:ngsi-ld:T:1", "type": "T"}]),
    ] {
        assert!(deserialize_exact::<Entity>(&refused).is_err(), "{refused}");
        assert!(Entity::from_node(&refused).is_err(), "{refused}");
    }
}
