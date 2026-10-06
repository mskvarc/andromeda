//! What the decode of a request and the clause 4.6.4 screening of what it
//! holds do with each shape of attribute node and each key: a node or a member
//! the specification does not admit is refused by the decode where it stands, a
//! key JSON-LD expansion would drop is refused by the screening, and everything
//! else is admitted.

use crate::compact_screening::{screen_compact_attribute, screen_compact_entity, screen_compact_patch};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
};
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
use andromeda_ngsi_ld_representation::compact_key_admission::CompactKeyError;
use pretty_assertions::assert_eq;
use serde_json::{Value, json, value::RawValue};

/// The refusal of a `type` token outside clause 5.2.4.
const NOT_AN_ATTRIBUTE_TYPE: &str = "declares a type that is not an NGSI-LD attribute type";

/// The refusal of a `type` that is not a string (Table 5.2.5-1).
const TYPE_NOT_A_STRING: &str = "declares a type that is not a string";

/// The refusal of a geometry type without coordinates (clause 4.7.1).
const GEOMETRY_WITHOUT_COORDINATES: &str = "names a GeoJSON geometry type without the member holding its coordinates";

/// The refusal of a typeless node carrying two families' value members.
const SEVERAL_VALUE_MEMBERS: &str = "declares no type and carries the value members of several attribute types";

/// The refusal of an array of instances and values (clause 4.5.5).
const MIXED_INSTANCES_AND_VALUES: &str = "mixes attribute instances with values in one array";

/// What the decode and the screening make of one payload.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// The decode refused the payload, in these words.
    Refused(String),
    /// The decode admitted the payload and the screening found no key to refuse.
    Admitted,
    /// The decode admitted the payload and the screening refused this key.
    Keyed(CompactKeyError),
}

/// The decode refusal of the node at `path`, for `fault`.
fn refused_at(path: &str, fault: &str) -> Outcome {
    Outcome::Refused(format!("the node at {path} is refused: the attribute {fault}"))
}

/// The decode refusal of the node at `path` carrying the seal `member`.
fn sealed_at(path: &str, member: &str) -> Outcome {
    refused_at(
        path,
        &format!("carries the member {member}, which is admitted only on the ngsildproof Property"),
    )
}

/// The decode refusal of the member at `path`, named by no attribute name.
fn attribute_name_refused_at(path: &str) -> Outcome {
    Outcome::Refused(format!("the node at {path} is refused: invalid attribute name: invalid NGSI-LD reference"))
}

/// The screening refusal of a keyword-form key at `path`.
fn keyword_form_at(path: &[PathSegment<'_>]) -> Outcome {
    Outcome::Keyed(CompactKeyError::KeywordFormKey {
        path: JsonPointer::from_walk(path),
    })
}

/// The screening refusal of an empty key at `path`.
fn empty_key_at(path: &[PathSegment<'_>]) -> Outcome {
    Outcome::Keyed(CompactKeyError::EmptyKey {
        path: JsonPointer::from_walk(path),
    })
}

/// The screening refusal of an empty type at `path`.
fn empty_type_at(path: &[PathSegment<'_>]) -> Outcome {
    Outcome::Keyed(CompactKeyError::EmptyType {
        path: JsonPointer::from_walk(path),
    })
}

/// A Building whose attributes are `members`.
fn entity(members: Value) -> Value {
    let mut object = json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"});
    if let (Some(target), Value::Object(members)) = (object.as_object_mut(), members) {
        target.extend(members);
    }
    object
}

/// The outcome of a screening that found nothing wrong or something.
fn screened(screening: Result<(), CompactKeyError>) -> Outcome {
    screening.map_or_else(Outcome::Keyed, |()| Outcome::Admitted)
}

/// What the request text of `payload` decodes and screens as, read as an Entity.
fn outcome(payload: &Value) -> Outcome {
    match <Entity>::from_request_text(payload.to_string().as_bytes()) {
        Ok(entity) => screened(screen_compact_entity(&entity)),
        Err(error) => Outcome::Refused(error.to_string()),
    }
}

/// What the request text of `payload` decodes and screens as, read as an
/// entity Fragment.
fn patch_outcome(payload: &Value) -> Outcome {
    match <EntityPatch>::from_request_text(payload.to_string().as_bytes()) {
        Ok(patch) => screened(screen_compact_patch(&patch)),
        Err(error) => Outcome::Refused(error.to_string()),
    }
}

/// What the request text of `payload` decodes and screens as, read as a merge
/// fragment.
fn merge_outcome(payload: &Value) -> Outcome {
    match <EntityPatch<MergeMember>>::from_request_text(payload.to_string().as_bytes()) {
        Ok(patch) => screened(screen_compact_patch(&patch)),
        Err(error) => Outcome::Refused(error.to_string()),
    }
}

/// What the request text of `node` decodes and screens as the Attribute
/// Fragment written to the attribute `name`.
fn attribute_outcome(name: &str, node: &Value) -> Outcome {
    let text = RawValue::from_string(node.to_string()).expect("the fixture is JSON");
    match <AttributeValue as MemberReading>::from_node(name, JsonPiece::new(&text)) {
        Ok(member) => screened(screen_compact_attribute(name, &member)),
        Err(error) => Outcome::Refused(error.to_string()),
    }
}

/// The payloads each breaking, or narrowly keeping, one of the rules at a
/// different depth and in a different shape, and what they are met with.
fn corpus() -> Vec<(Value, Outcome)> {
    [seal_cases(), node_cases(), key_cases()].into_iter().flatten().collect()
}

/// The corpus payloads that carry the seals of clause 4.5.2.2.
fn seal_cases() -> Vec<(Value, Outcome)> {
    vec![
        (
            entity(json!({
                "name": {"type": "Property", "value": "x", "unitCode": "CEL", "observedAt": "2025-01-01T00:00:00Z"},
                "owner": {"type": "Relationship", "object": "urn:ngsi-ld:P:1"},
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
                "street": {"type": "LanguageProperty", "languageMap": {"en": "x"}},
                "config": {"type": "JsonProperty", "json": {"@context": "data, not a context"}},
                "list": {"type": "ListProperty", "valueList": [1, {"a": 2}]},
                "stops": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:S:1"]},
                "kind": {"type": "VocabProperty", "vocab": "x"}
            })),
            Outcome::Admitted,
        ),
        (
            entity(json!({"belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityIdSealed": "urn:ngsi-ld:Building:1"}})),
            sealed_at("/belongsTo", "entityIdSealed"),
        ),
        (
            entity(json!({"belongsTo": {"object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Building"}})),
            sealed_at("/belongsTo", "entityTypeSealed"),
        ),
        (
            entity(
                json!({"ngsildproof": {"type": "Property", "value": {"cryptosuite": "eddsa-jcs-2022"}, "entityIdSealed": "urn:x:1", "entityTypeSealed": "Building"}}),
            ),
            Outcome::Admitted,
        ),
        (
            entity(json!({"ngsildproof": {"type": "Relationship", "object": "urn:x:1", "entityIdSealed": "urn:x:1"}})),
            sealed_at("/ngsildproof", "entityIdSealed"),
        ),
        (
            entity(json!({"temperature": {"type": "NotAnAttributeType", "value": 21.5}})),
            refused_at("/temperature", NOT_AN_ATTRIBUTE_TYPE),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 0.5, "entityIdSealed": "x"}}})),
            sealed_at("/speed/accuracy", "entityIdSealed"),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"value": 0.5, "entityTypeSealed": "x"}}})),
            sealed_at("/speed/accuracy", "entityTypeSealed"),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "readings": [{"value": 1}, 7, {"value": 2, "entityIdSealed": "x"}]}})),
            refused_at("/speed/readings", MIXED_INSTANCES_AND_VALUES),
        ),
        (
            entity(
                json!({"speed": {"type": "Property", "value": 1, "odd": {"type": "Bogus", "deeper": {"type": "Property", "value": 1, "entityIdSealed": "x"}}}}),
            ),
            refused_at("/speed/odd", NOT_AN_ATTRIBUTE_TYPE),
        ),
        (
            entity(json!({"speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 2, "datasetId": "urn:ds:1", "entityTypeSealed": "x"}]})),
            sealed_at("/speed/1", "entityTypeSealed"),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "entityTypeSealed": "x", "entityIdSealed": "y"}})),
            sealed_at("/speed", "entityIdSealed"),
        ),
        // The rule over the compact JSON stepped over a sub-attribute named
        // `name`; the decode reads it as the attribute clause 4.5.2.2 makes it,
        // seals included.
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "name": {"type": "Property", "value": 1, "entityIdSealed": "x"}}})),
            sealed_at("/speed/name", "entityIdSealed"),
        ),
    ]
}

/// The corpus payloads that carry a node no clause admits where an attribute
/// stands, and the one that narrowly keeps a `type` inside a value.
fn node_cases() -> Vec<(Value, Outcome)> {
    vec![
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Propertyy", "value": 0.5}}})),
            refused_at("/speed/accuracy", NOT_AN_ATTRIBUTE_TYPE),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": 7, "value": 0.5}}})),
            refused_at("/speed/accuracy", TYPE_NOT_A_STRING),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "spot": {"type": "Point"}}})),
            refused_at("/speed/spot", GEOMETRY_WITHOUT_COORDINATES),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "both": {"object": "urn:x:1", "vocab": "x"}}})),
            refused_at("/speed/both", SEVERAL_VALUE_MEMBERS),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "readings": [{"value": 1, "datasetId": "urn:ds:1"}, 7]}})),
            refused_at("/speed/readings", MIXED_INSTANCES_AND_VALUES),
        ),
        (
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": {"type": "Feature"}}}})),
            Outcome::Admitted,
        ),
    ]
}

/// The corpus payloads that stand keys clause 4.6.4 refuses or admits.
fn key_cases() -> Vec<(Value, Outcome)> {
    vec![
        (
            entity(json!({"p": {"type": "Property", "value": {"opening hours": "9-5", "id": "abc", "type": "Postal Address"}}})),
            Outcome::Admitted,
        ),
        (
            entity(json!({"p": {"type": "Property", "value": {"@foo": "x", "name": "y"}}})),
            keyword_form_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("@foo")]),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": 1, "sub": {"value": {"a": [{"@Bar": 1}]}}}})),
            keyword_form_at(&[
                PathSegment::Member("p"),
                PathSegment::Member("sub"),
                PathSegment::Member("value"),
                PathSegment::Member("a"),
                PathSegment::Index(0),
                PathSegment::Member("@Bar"),
            ]),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": {"": "x"}}})),
            empty_key_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("")]),
        ),
        (
            entity(json!({"p": {"type": "ListProperty", "valueList": [1, {"": 2}]}})),
            empty_key_at(&[
                PathSegment::Member("p"),
                PathSegment::Member("valueList"),
                PathSegment::Index(1),
                PathSegment::Member(""),
            ]),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": {"type": ""}}})),
            empty_type_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("type")]),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": {"@type": ["A", ""]}}, "q": {"type": "Property", "value": {"@foo": 1}}})),
            empty_type_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("@type")]),
        ),
        (
            entity(json!({"label": {"type": "LanguageProperty", "languageMap": {"en": "x", "@none": "y"}}})),
            Outcome::Admitted,
        ),
    ]
}

/// Each payload of the corpus is met with the refusal of the decode, or the
/// refusal or admission of the screening, the rules recorded for it.
#[test]
fn each_payload_of_the_corpus_reaches_the_recorded_outcome() {
    for (payload, expected) in corpus() {
        assert_eq!(outcome(&payload), expected, "{payload}");
    }
}

/// The corpus is not vacuous: it holds payloads the decode refuses, payloads
/// the key rule refuses and payloads both admit.
#[test]
fn the_corpus_holds_refusals_of_both_rules_and_admissions() {
    let outcomes: Vec<Outcome> = corpus().into_iter().map(|(_, expected)| expected).collect();
    assert!(outcomes.iter().any(|found| matches!(found, Outcome::Refused(_))));
    assert!(outcomes.iter().any(|found| matches!(found, Outcome::Keyed(_))));
    assert!(outcomes.iter().any(|found| matches!(found, Outcome::Admitted)));
}

/// An entity Fragment, and a merge fragment, are decoded and screened as the
/// entity they patch, their three geo-properties included.
#[test]
fn a_fragment_is_screened_as_the_entity_it_patches() {
    for (payload, _) in corpus() {
        let entity = outcome(&payload);
        assert_eq!(patch_outcome(&payload), entity, "{payload}");
        assert_eq!(merge_outcome(&payload), entity, "{payload}");
    }
}

/// A bare Attribute Fragment is screened from the attribute's name.
#[test]
fn an_attribute_fragment_is_screened_from_its_name() {
    assert_eq!(
        attribute_outcome("p", &json!({"type": "Property", "value": {"@foo": 1}})),
        keyword_form_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("@foo")])
    );
}

/// Clause 4.5.2.3: a bare Attribute Fragment is read as the node itself.
#[test]
fn a_single_attribute_fragment_is_read_as_the_node_itself() {
    assert_eq!(attribute_outcome("speed", &json!({"value": 99})), Outcome::Admitted);
    assert_eq!(
        attribute_outcome("speed", &json!({"type": "Relationship", "object": "urn:ngsi-ld:X:1"})),
        Outcome::Admitted
    );
    assert_eq!(
        attribute_outcome("speed", &json!({"type": "Propertyy", "value": 1})),
        Outcome::Refused(format!("the node at / is refused: the attribute {NOT_AN_ATTRIBUTE_TYPE}"))
    );
}

/// Clause 5.2.4: every one of the eight attribute types is an attribute type,
/// read with the value member its own clause gives it.
#[test]
fn every_clause_5_2_4_attribute_type_is_admitted() {
    for attribute_type in AttributeType::ALL {
        let node = match attribute_type {
            AttributeType::Property => json!({"type": "Property", "value": 1}),
            AttributeType::GeoProperty => json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}),
            AttributeType::ListProperty => json!({"type": "ListProperty", "valueList": [1]}),
            AttributeType::Relationship => json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1"}),
            AttributeType::LanguageProperty => json!({"type": "LanguageProperty", "languageMap": {"en": "a"}}),
            AttributeType::VocabProperty => json!({"type": "VocabProperty", "vocab": "x"}),
            AttributeType::JsonProperty => json!({"type": "JsonProperty", "json": {"a": 1}}),
            AttributeType::ListRelationship => json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1"]}),
        };
        assert_eq!(outcome(&entity(json!({"speed": node}))), Outcome::Admitted, "{attribute_type}");
    }
}

/// Entity members, concise values and geometries are no attribute
/// declarations; a payload that is no object is no entity.
#[test]
fn entity_members_concise_values_and_geometries_are_admitted_and_a_non_object_is_refused() {
    let payload = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "scope": "/a",
        "speed": 42,
        "location": {"type": "Point", "coordinates": [1.0, 2.0]},
        "raw": {"other": 7, "value": 1}
    });
    assert_eq!(outcome(&payload), Outcome::Admitted);
    assert!(matches!(outcome(&json!("not an object")), Outcome::Refused(_)));
}

/// Clause 4.5.5: a multi-instance array is read instance by instance, each
/// refusal located at its element.
#[test]
fn a_multi_instance_array_is_read_instance_by_instance() {
    let payload = entity(json!({"speed": [
        {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"},
        {"type": "Bogus", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:b"}
    ]}));
    assert_eq!(outcome(&payload), refused_at("/speed/1", NOT_AN_ATTRIBUTE_TYPE));
}

/// Clause 4.5.2.2 makes a sub-attribute "the result of serializing a
/// Property", so a concise instance's sub-attribute is held to clause 5.2.4
/// where it stands.
#[test]
fn a_sub_attribute_of_a_concise_instance_is_read_too() {
    let payload = entity(json!({"speed": {"value": 1, "accuracy": {"type": "Feature", "x": 1}}}));
    assert_eq!(outcome(&payload), refused_at("/speed/accuracy", NOT_AN_ATTRIBUTE_TYPE));
}

/// Clause 4.5.2.3 reads an object carrying a `type` as a normalized attribute,
/// whose `type` Table 5.2.5-1 makes a String.
#[test]
fn a_type_that_is_not_a_string_is_refused_at_every_depth() {
    assert_eq!(outcome(&entity(json!({"raw": {"type": 7, "value": 1}}))), refused_at("/raw", TYPE_NOT_A_STRING));
    assert_eq!(
        outcome(&entity(
            json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": ["Property"], "value": 0.5}}})
        )),
        refused_at("/speed/accuracy", TYPE_NOT_A_STRING)
    );
}

/// Clause 4.7.1: a geometry type without its coordinates is no geometry.
#[test]
fn a_geometry_type_without_coordinates_is_refused() {
    assert_eq!(
        outcome(&entity(json!({"location": {"type": "Point"}}))),
        refused_at("/location", GEOMETRY_WITHOUT_COORDINATES)
    );
}

/// Clause 4.5.2.3: a typeless node names its family by its value member, so
/// two families' value members name none.
#[test]
fn two_value_members_on_a_typeless_attribute_are_refused() {
    assert_eq!(
        outcome(&entity(json!({"ambiguous": {"object": "urn:ngsi-ld:X:1", "languageMap": {"en": "x"}}}))),
        refused_at("/ambiguous", SEVERAL_VALUE_MEMBERS)
    );
}

/// Clause 4.5.5 writes several instances as an array of instances; an array
/// holding instances and values as well is neither that nor one value.
#[test]
fn an_array_mixing_instances_and_values_is_refused() {
    assert_eq!(
        outcome(&entity(json!({"speed": [{"value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"}, 30]}))),
        refused_at("/speed", MIXED_INSTANCES_AND_VALUES)
    );
    assert_eq!(
        outcome(&entity(json!({"speed": [{"type": "Feature"}, 30]}))),
        refused_at("/speed/0", NOT_AN_ATTRIBUTE_TYPE)
    );
}

/// A value is data: a `type` member inside a Property's `value`, or inside an
/// object that is itself an implicit Property value, declares nothing.
#[test]
fn a_type_member_inside_a_value_is_data() {
    let payload = entity(json!({
        "declared": {"type": "Property", "value": {"type": "Feature", "x": 1}},
        "implicit": {"a": {"type": "Feature", "x": 1}},
        "values": [{"a": {"type": "Feature"}}, 1]
    }));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// Clause 4.5.2.2: `entityIdSealed` and `entityTypeSealed` "shall never be
/// present, unless the Property name is `ngsildproof`".
#[test]
fn a_data_integrity_seal_on_an_attribute_that_is_not_ngsildproof_is_refused() {
    for member in ["entityIdSealed", "entityTypeSealed"] {
        let payload = entity(json!({"speed": {"type": "Property", "value": 1, member: "urn:ngsi-ld:Vehicle:1"}}));
        assert_eq!(outcome(&payload), sealed_at("/speed", member), "{member}");
    }
}

/// Clause 4.5.3.2, "Prohibited": on a Relationship in the normalized
/// representation, "`entityIdSealed` and `entityTypeSealed` shall never be
/// present", without the `ngsildproof` exception clause 4.5.2.2 grants a
/// Property.
#[test]
fn a_data_integrity_seal_on_a_normalized_relationship_is_refused() {
    for member in ["entityIdSealed", "entityTypeSealed"] {
        let payload = entity(json!({"belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Vehicle:1"}}));
        assert_eq!(outcome(&payload), sealed_at("/belongsTo", member), "{member}");
    }
}

/// Clause 4.5.3.3 repeats the same prohibition for the concise representation,
/// where a Relationship declares no `type` and its family is inferred from
/// `object`.
#[test]
fn a_data_integrity_seal_on_a_concise_relationship_is_refused_through_family_inference() {
    for member in ["entityIdSealed", "entityTypeSealed"] {
        let payload = entity(json!({"belongsTo": {"object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Vehicle:1"}}));
        assert_eq!(outcome(&payload), sealed_at("/belongsTo", member), "{member}");
    }
}

/// The name alone does not admit a seal: clauses 4.5.3.2 and 4.5.3.3 prohibit
/// both members on a Relationship with no exception, and clause 4.5.2.2 defines
/// `ngsildproof` as a Property.
#[test]
fn a_seal_on_an_ngsildproof_that_is_a_relationship_is_refused_in_both_representations() {
    for node in [
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:ngsi-ld:Vehicle:1"}),
        json!({"object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:ngsi-ld:Vehicle:1"}),
    ] {
        assert_eq!(outcome(&entity(json!({"ngsildproof": node}))), sealed_at("/ngsildproof", "entityIdSealed"));
    }
}

/// A seal forged on a concise Relationship written as a sub-attribute is
/// refused too: clause 4.5.2.2 makes a sub-attribute an attribute.
#[test]
fn a_seal_on_a_concise_relationship_sub_attribute_is_refused() {
    let payload = entity(json!({"speed": {
        "type": "Property",
        "value": 1,
        "providedBy": {"object": "urn:ngsi-ld:Device:1", "entityTypeSealed": "Vehicle"}
    }}));
    assert_eq!(outcome(&payload), sealed_at("/speed/providedBy", "entityTypeSealed"));
}

/// The same two members under the `ngsildproof` Property clause 4.5.2.2
/// defines them for are the intended shape and are admitted, below an
/// attribute too.
#[test]
fn the_seals_of_the_ngsildproof_property_are_admitted() {
    let payload = entity(json!({"speed": {
        "type": "Property",
        "value": 1,
        "ngsildproof": {
            "type": "Property",
            "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022"},
            "entityIdSealed": "urn:ngsi-ld:Vehicle:1",
            "entityTypeSealed": "Vehicle"
        }
    }}));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// A seal is refused however deep it is buried, because a sub-attribute is
/// itself an attribute (clause 4.5.2.2) and carries the same prohibition.
#[test]
fn a_seal_on_a_sub_attribute_is_refused_under_the_sub_attributes_name() {
    let payload = entity(json!({"speed": {
        "type": "Property",
        "value": 1,
        "accuracy": {"type": "Property", "value": 0.5, "entityIdSealed": "urn:ngsi-ld:Vehicle:1"}
    }}));
    assert_eq!(outcome(&payload), sealed_at("/speed/accuracy", "entityIdSealed"));
}

/// A Property's value is a value, not a nested attribute, so a member named
/// like a seal inside it is data rather than a forged seal.
#[test]
fn a_seal_name_inside_a_property_value_is_not_a_seal() {
    let payload = entity(json!({"speed": {"type": "Property", "value": {"entityIdSealed": "urn:ngsi-ld:Vehicle:1"}}}));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// Clause 5.5.7, closing note: a `JsonProperty` "is designed to hold native
/// JSON values which are by definition not available for expansion and
/// compaction via an @context", so a key spelled `@context` inside its `json`
/// member is data, as is any other key there.
#[test]
fn any_key_inside_a_json_property_is_data_and_is_admitted() {
    let payload = entity(json!({"config": {"type": "JsonProperty", "json": {
        "@context": {"anything": "at all"},
        "nested": {"@context": "https://example.org/ctx.jsonld"},
        "@foo": 1,
        "": {"type": ""}
    }}}));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// The core context coerces `previousJson`, `contextSourceExtras` and
/// `problemDetails` to `@type: "@json"` exactly as it does `json`, so the same
/// exemption reaches all four wherever they stand.
#[test]
fn every_core_native_json_member_is_exempt_at_any_depth() {
    for member in ["json", "previousJson", "contextSourceExtras", "problemDetails"] {
        let payload = entity(json!({"config": {"type": "JsonProperty", "json": {"ok": true}, member: {"@foo": {"": 1}}}}));
        assert_eq!(outcome(&payload), Outcome::Admitted, "{member}");
    }
}

/// A `json` member standing inside a Property value is still the core `json`
/// term, whose content the processor keeps verbatim.
#[test]
fn a_keyword_form_key_under_a_json_member_nested_in_a_value_is_admitted() {
    let payload = entity(json!({"speed": {"type": "Property", "value": {"json": {"@foo": {}}}}}));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// An ordinary entity, with every attribute family the eight of clause 5.2.4
/// admit, passes untouched; so does a structured value whose keys are no
/// attribute names, which clause 4.6.2 does not restrict.
#[test]
fn an_entity_carrying_no_refused_key_is_admitted() {
    let payload = entity(json!({
        "scope": "/a/b",
        "speed": {"type": "Property", "value": 42, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z"},
        "address": {"type": "Property", "value": {"opening hours": "9-5", "id": "abc", "type": "Postal Address", "@id": "x", "@type": "y"}},
        "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1"},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        "label": {"type": "LanguageProperty", "languageMap": {"en": "hello", "de": "hallo", "@none": "hi"}},
        "category": {"type": "VocabProperty", "vocab": "Truck"},
        "config": {"type": "JsonProperty", "json": {"a": [1, 2, {"b": true}]}},
        "readings": {"type": "ListProperty", "valueList": [1, 2, 3]},
        "sites": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Site:1"]}
    }));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// JSON-LD 1.1 expansion ignores a key that has the form of a keyword without
/// being one, so the member would vanish (clause 4.6.4); stood as an attribute
/// or a sub-attribute name it is no attribute name, and below a value it is
/// refused by the screening.
#[test]
fn a_keyword_form_key_is_refused_at_every_depth() {
    for (payload, expected) in [
        (entity(json!({"@foo": 1})), attribute_name_refused_at("/@foo")),
        (
            entity(json!({"p": {"type": "Property", "value": 1, "@foo": 1}})),
            attribute_name_refused_at("/p/@foo"),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": {"@foo": "x", "name": "y"}}})),
            keyword_form_at(&[PathSegment::Member("p"), PathSegment::Member("value"), PathSegment::Member("@foo")]),
        ),
        (
            entity(json!({"p": {"type": "Property", "value": [{"a": {"@Bar": 1}}]}})),
            keyword_form_at(&[
                PathSegment::Member("p"),
                PathSegment::Member("value"),
                PathSegment::Index(0),
                PathSegment::Member("a"),
                PathSegment::Member("@Bar"),
            ]),
        ),
    ] {
        assert_eq!(outcome(&payload), expected, "{payload}");
    }
}

/// A key starting with `@` that is no keyword form (it holds more than
/// letters) is an ordinary term expansion keeps.
#[test]
fn a_key_starting_with_an_at_sign_but_holding_more_than_letters_is_admitted() {
    let payload = entity(json!({"p": {"type": "Property", "value": {"@foo-bar": 1, "@1": 2}}}));
    assert_eq!(outcome(&payload), Outcome::Admitted);
}

/// The keys of a language map are language tags, which the language rules
/// read; neither key rule applies to them, so a key that is no language tag is
/// refused by the decode as one, not by the screening as a term.
#[test]
fn the_keys_of_a_language_map_are_read_as_language_tags() {
    let payload = entity(json!({"label": {"type": "LanguageProperty", "languageMap": {"": "x", "@foo": "y"}}}));
    assert_eq!(
        outcome(&payload),
        Outcome::Refused(
            "the node at /label/languageMap is refused: invalid 'languageMap' member: invalid BCP47 language tag: invalid langtag:  at line 1 column 3"
                .to_owned()
        )
    );
}

/// A bare Attribute Fragment reports a refused key under the attribute name
/// the request URI carries, through its instances too. A typeless object is
/// the `value` of an implicit Property (clause 4.5.2.3), which is where the
/// key stands.
#[test]
fn an_attribute_fragment_is_located_under_its_attribute_name() {
    assert_eq!(
        attribute_outcome(
            "speed",
            &json!({"type": "Property", "value": 42, "accuracy": {"type": "Property", "value": 0.5, "@foo": 1}})
        ),
        attribute_name_refused_at("/accuracy/@foo")
    );
    assert_eq!(
        attribute_outcome("speed", &json!([{"type": "Property", "value": 42, "unit": {"": 1}}])),
        empty_key_at(&[
            PathSegment::Member("speed"),
            PathSegment::Index(0),
            PathSegment::Member("unit"),
            PathSegment::Member("value"),
            PathSegment::Member(""),
        ])
    );
}
