//! The "Prohibited" lists of the family clauses (ETSI GS CIM 009 v1.9.1 clauses
//! 4.5.2.2 and 4.5.2.3, 4.5.3.2 and 4.5.3.3, 4.5.18.2 and 4.5.18.3, 4.5.20.2
//! and 4.5.20.3, 4.5.21.2 and 4.5.21.3, 4.5.22.2 and 4.5.22.3, 4.5.24.2 and
//! 4.5.24.3), held against the decode of an attribute: a family refuses each
//! member its list prohibits, where the instance stands, and admits the value
//! member it declares for itself and the members every family shares.
//!
//! Each list is written out member by member below, as the clause words it, and
//! none is derived from the table the decode reads.

use crate::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use pretty_assertions::assert_eq;
use serde_json::{Value, json, value::RawValue};

/// Every member some family claims for its value or for its inline linked
/// entities: the seven value members, their output-only `previous` spellings
/// and the two members of clause 4.5.23.2.
const CLAIMED: [&str; 16] = [
    "entity",
    "entityList",
    "json",
    "languageMap",
    "object",
    "objectList",
    "previousJson",
    "previousLanguageMap",
    "previousObject",
    "previousObjectList",
    "previousValue",
    "previousValueList",
    "previousVocab",
    "value",
    "valueList",
    "vocab",
];

/// Clauses 4.5.2.2 and 4.5.2.3, "Prohibited", less the seals: what a Property
/// shall never include.
const PROPERTY_PROHIBITED: [&str; 14] = [
    "entity",
    "entityList",
    "json",
    "previousJson",
    "languageMap",
    "previousLanguageMap",
    "object",
    "previousObject",
    "objectList",
    "previousObjectList",
    "valueList",
    "previousValueList",
    "vocab",
    "previousVocab",
];

/// Clauses 4.5.3.2 and 4.5.3.3, "Prohibited", less the seals and the
/// `unitCode` entry: what a Relationship shall never include.
const RELATIONSHIP_PROHIBITED: [&str; 13] = [
    "entityList",
    "languageMap",
    "previousLanguageMap",
    "json",
    "previousJson",
    "objectList",
    "previousObjectList",
    "value",
    "previousValue",
    "valueList",
    "previousValueList",
    "vocab",
    "previousVocab",
];

/// Clauses 4.5.18.2 and 4.5.18.3: a `LanguageProperty` is a Property whose
/// `languageMap` replaces `value`, "as value is a generalization of
/// languageMap".
const LANGUAGE_PROPERTY_PROHIBITED: [&str; 14] = [
    "entity",
    "entityList",
    "json",
    "previousJson",
    "object",
    "previousObject",
    "objectList",
    "previousObjectList",
    "value",
    "previousValue",
    "valueList",
    "previousValueList",
    "vocab",
    "previousVocab",
];

/// Clauses 4.5.20.2 and 4.5.20.3: a `VocabProperty` is a Property whose
/// `vocab` replaces `value`, "as value is a generalization of vocab".
const VOCAB_PROPERTY_PROHIBITED: [&str; 14] = [
    "entity",
    "entityList",
    "json",
    "previousJson",
    "languageMap",
    "previousLanguageMap",
    "object",
    "previousObject",
    "objectList",
    "previousObjectList",
    "value",
    "previousValue",
    "valueList",
    "previousValueList",
];

/// Clauses 4.5.24.2 and 4.5.24.3: a `JsonProperty` is a Property whose `json`
/// replaces `value`, "as value is a generalization of json".
const JSON_PROPERTY_PROHIBITED: [&str; 14] = [
    "entity",
    "entityList",
    "languageMap",
    "previousLanguageMap",
    "object",
    "previousObject",
    "objectList",
    "previousObjectList",
    "value",
    "previousValue",
    "valueList",
    "previousValueList",
    "vocab",
    "previousVocab",
];

/// Clauses 4.5.21.2 and 4.5.21.3: a `ListProperty` is a Property whose
/// `valueList` replaces `value`, "as value is a generalization of valueList".
const LIST_PROPERTY_PROHIBITED: [&str; 14] = [
    "entity",
    "entityList",
    "json",
    "previousJson",
    "languageMap",
    "previousLanguageMap",
    "object",
    "previousObject",
    "objectList",
    "previousObjectList",
    "value",
    "previousValue",
    "vocab",
    "previousVocab",
];

/// Clauses 4.5.22.2 and 4.5.22.3: a `ListRelationship` is a Relationship whose
/// `objectList` replaces `object` and whose `entityList` replaces `entity`,
/// "as object is a generalization of objectList" and "entity is a
/// generalization of entityList".
const LIST_RELATIONSHIP_PROHIBITED: [&str; 13] = [
    "entity",
    "json",
    "languageMap",
    "previousLanguageMap",
    "previousJson",
    "object",
    "previousObject",
    "value",
    "previousValue",
    "valueList",
    "previousValueList",
    "vocab",
    "previousVocab",
];

/// A geometry, the value of a `GeoProperty`.
fn point() -> Value {
    json!({"type": "Point", "coordinates": [1.0, 2.0]})
}

/// An inline linked entity (clause 4.5.23.2).
fn linked_entity() -> Value {
    json!({"id": "urn:ngsi-ld:A:1", "type": "A"})
}

/// A node of `family` carrying only the value member the family declares for
/// itself.
fn own_node(family: AttributeType) -> Value {
    match family {
        AttributeType::Property => json!({"type": "Property", "value": 1}),
        AttributeType::GeoProperty => json!({"type": "GeoProperty", "value": point()}),
        AttributeType::ListProperty => json!({"type": "ListProperty", "valueList": [1]}),
        AttributeType::Relationship => json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1"}),
        AttributeType::LanguageProperty => json!({"type": "LanguageProperty", "languageMap": {"en": "a"}}),
        AttributeType::VocabProperty => json!({"type": "VocabProperty", "vocab": "x"}),
        AttributeType::JsonProperty => json!({"type": "JsonProperty", "json": {"a": 1}}),
        AttributeType::ListRelationship => json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1"]}),
    }
}

/// A value `member` is decoded from when `family` declares it.
fn admitted_value(family: AttributeType, member: &str) -> Value {
    match member {
        "value" | "previousValue" if family == AttributeType::GeoProperty => point(),
        "value" | "previousValue" => json!(1),
        "object" | "previousObject" => json!("urn:ngsi-ld:A:1"),
        "entity" => linked_entity(),
        "languageMap" | "previousLanguageMap" => json!({"en": "a"}),
        "vocab" | "previousVocab" => json!("x"),
        "json" | "previousJson" => json!({"a": 1}),
        "valueList" | "previousValueList" => json!([1]),
        "objectList" | "previousObjectList" => json!(["urn:ngsi-ld:A:1"]),
        "entityList" => json!([linked_entity()]),
        other => panic!("{other} is no member a family claims"),
    }
}

/// `node` with the member `member` set to `value`.
fn with_member(node: Value, member: &str, value: Value) -> Value {
    let Value::Object(mut members) = node else {
        panic!("a node is an object: {node}");
    };
    members.insert(member.to_owned(), value);
    Value::Object(members)
}

/// What decoding `node` as the attribute `a` gives: nothing, or the words of
/// its refusal.
fn decoded(node: &Value) -> Result<(), String> {
    let text = RawValue::from_string(node.to_string()).expect("the fixture is JSON");
    <AttributeValue as MemberReading>::from_node("a", JsonPiece::new(&text))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The refusal of an instance of `family` carrying `member`.
fn prohibited(family: AttributeType, member: &str) -> String {
    format!(
        "the node at / is refused: the attribute carries the member {member}, which defines the value of another attribute family and is never present on a {family}"
    )
}

/// Decode an instance of `family` carrying each claimed member in turn, and
/// assert it is refused exactly for the members of `prohibited_members`.
fn assert_family_prohibits(family: AttributeType, prohibited_members: &[&str]) {
    for member in CLAIMED {
        let outcome = if prohibited_members.contains(&member) {
            decoded(&with_member(own_node(family), member, json!(1)))
        } else {
            decoded(&with_member(own_node(family), member, admitted_value(family, member)))
        };
        let expected = if prohibited_members.contains(&member) {
            Err(prohibited(family, member))
        } else {
            Ok(())
        };
        assert_eq!(outcome, expected, "{family}/{member}");
    }
}

/// Clauses 4.5.2.2 and 4.5.2.3, "Prohibited", verbatim.
#[test]
fn a_property_refuses_exactly_the_members_clause_4_5_2_2_lists() {
    assert_family_prohibits(AttributeType::Property, &PROPERTY_PROHIBITED);
}

/// Clause 4.7.3 defines a `GeoProperty` by the Property of clause 4.5.2.2, its
/// geometry in place of the value, so the Property list holds.
#[test]
fn a_geo_property_refuses_exactly_the_members_a_property_does() {
    assert_family_prohibits(AttributeType::GeoProperty, &PROPERTY_PROHIBITED);
}

/// Clauses 4.5.3.2 and 4.5.3.3, "Prohibited", less the `unitCode` entry the
/// unit admission owns.
#[test]
fn a_relationship_refuses_exactly_the_members_clause_4_5_3_2_lists() {
    assert_family_prohibits(AttributeType::Relationship, &RELATIONSHIP_PROHIBITED);
}

/// Clause 4.5.18.2: a `LanguageProperty` refuses `value` and every other
/// family's value member, and keeps `languageMap`.
#[test]
fn a_language_property_refuses_value_and_every_other_familys_value_member() {
    assert_family_prohibits(AttributeType::LanguageProperty, &LANGUAGE_PROPERTY_PROHIBITED);
}

/// Clause 4.5.20.2 for a `VocabProperty` and clause 4.5.24.2 for a
/// `JsonProperty`: each refuses `value` and keeps its own specialization.
#[test]
fn the_vocab_and_json_families_refuse_value_and_keep_their_own_specialized_member() {
    assert_family_prohibits(AttributeType::VocabProperty, &VOCAB_PROPERTY_PROHIBITED);
    assert_family_prohibits(AttributeType::JsonProperty, &JSON_PROPERTY_PROHIBITED);
}

/// Clause 4.5.21.2: `value` is a generalization of `valueList`, so a
/// `ListProperty` refuses it while keeping the `unitCode` clause 4.5.2.2 gives
/// every Property.
#[test]
fn a_list_property_refuses_the_generalized_value_but_not_its_inherited_unit_code() {
    assert_family_prohibits(AttributeType::ListProperty, &LIST_PROPERTY_PROHIBITED);
    assert_eq!(decoded(&with_member(own_node(AttributeType::ListProperty), "unitCode", json!("KMH"))), Ok(()));
}

/// Clause 4.5.22.2: `object` generalizes `objectList` and `entity`
/// generalizes `entityList`, so a `ListRelationship` refuses both
/// generalizations and keeps both specializations.
#[test]
fn a_list_relationship_refuses_the_generalized_object_and_entity_members() {
    assert_family_prohibits(AttributeType::ListRelationship, &LIST_RELATIONSHIP_PROHIBITED);
}

/// Every family admits its own value member and its own output-only spelling.
#[test]
fn every_family_admits_its_own_value_member_and_its_own_output_only_spelling() {
    for family in AttributeType::ALL {
        let node = with_member(own_node(family), family.previous_key(), admitted_value(family, family.previous_key()));
        assert_eq!(decoded(&node), Ok(()), "{family}");
        assert!(CLAIMED.contains(&family.compact_value_key()), "{family}");
        assert!(CLAIMED.contains(&family.previous_key()), "{family}");
    }
}

/// A sub-attribute name and the members every family shares are admitted by
/// every family.
#[test]
fn a_sub_attribute_name_and_the_shared_members_are_admitted_by_every_family() {
    for family in AttributeType::ALL {
        let node = [
            ("speed", json!(1)),
            ("observedAt", json!("2024-01-01T00:00:00Z")),
            ("datasetId", json!("urn:ngsi-ld:Dataset:a")),
            ("instanceId", json!("urn:ngsi-ld:attribute:instance:1")),
            ("createdAt", json!("2024-01-01T00:00:00Z")),
            ("expiresAt", json!("2030-01-01T00:00:00Z")),
        ]
        .into_iter()
        .fold(own_node(family), |node, (member, value)| with_member(node, member, value));
        assert_eq!(decoded(&node), Ok(()), "{family}");
    }
}

/// The prohibition holds where the request carries the attribute: in an entity,
/// an entity Fragment and a merge fragment, below the top level too.
#[test]
fn a_prohibited_member_is_refused_in_an_entity_a_fragment_and_a_merge_fragment() {
    let payloads = [
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "temperature": {"type": "Property", "value": 21.5, "object": "urn:ngsi-ld:Site:1"}}),
        json!({"id": "urn:ngsi-ld:T:1", "type": "T", "temperature": {"type": "Property", "value": 21.5, "accuracy": {"type": "Property", "value": 1, "object": "urn:ngsi-ld:Site:1"}}}),
    ];
    for (payload, at) in payloads.iter().zip(["/temperature", "/temperature/accuracy"]) {
        let text = payload.to_string();
        let expected = format!(
            "the node at {at} is refused: the attribute carries the member object, which defines the value of another attribute family and is never present on a Property"
        );
        assert_eq!(
            <Entity>::from_request_text(text.as_bytes()).map(|_| ()).map_err(|error| error.to_string()),
            Err(expected.clone()),
            "{payload}"
        );
        assert_eq!(
            <EntityPatch>::from_request_text(text.as_bytes()).map(|_| ()).map_err(|error| error.to_string()),
            Err(expected.clone()),
            "{payload}"
        );
        assert_eq!(
            <EntityPatch<MergeMember>>::from_request_text(text.as_bytes())
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Err(expected),
            "{payload}"
        );
    }
}
