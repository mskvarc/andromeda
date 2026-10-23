//! The reading of attribute members from a compact node: every node written
//! where an attribute stands is read into a typed member the specification
//! defines (clauses 4.5.2.3, 4.5.5, 4.7.3 and their siblings) or refused where
//! it stands (clause 5.5.4).

use crate::{
    attribute::{
        geo_property_value::GeoPropertyValue,
        instance::{Attribute, AttributeValue},
        member_reading::MemberReading,
        value::NgsiLdValue,
    },
    entity::Entity,
    node_refusal::{NodeReadError, NodeRefusal, RefusedNode},
    request_body::RequestBody,
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, time::rendering::instant_text};
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::AttributeNodeFault,
    compact_node::aliased_node::AliasedNode,
    core_term_aliases::CoreTermAliases,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json, value::RawValue};
use std::{convert::Infallible, thread};

/// The member `member` reads as under the name `name`, read from its text.
fn read(name: &str, member: &Value) -> Result<AttributeValue, NodeReadError<Infallible>> {
    let text = RawValue::from_string(member.to_string()).expect("the member is JSON");
    <AttributeValue as MemberReading>::from_node(name, JsonPiece::new(&text))
}

/// The member as the JSON it renders back to.
fn rendered(member: &AttributeValue) -> Value {
    serde_json::to_value(member).expect("an attribute member renders")
}

/// The one instance a member read as.
fn single(member: &Value) -> Attribute {
    match read("member", member) {
        Ok(AttributeValue::Single(instance)) => *instance,
        other => panic!("one instance: {other:?} from {member}"),
    }
}

/// The refusal a member reads as.
fn refused(member: &Value) -> RefusedNode {
    match read("member", member) {
        Err(NodeReadError::Refused(refused)) => refused,
        Ok(read) => panic!("refused, not {read:?}: {member}"),
        Err(NodeReadError::Unreadable(never)) => match never {},
    }
}

/// A JSON literal of `text`.
fn literal(text: &str) -> JsonLiteral {
    JsonLiteral::admit(text).expect("a literal")
}

/// The value a Property instance holds.
fn property_value(instance: &Attribute) -> &NgsiLdValue {
    let Attribute::Property(property) = instance else {
        panic!("a Property: {instance:?}");
    };
    &property.value
}

/// A bare scalar is a concise Property (clause 4.5.2.3), at the top level
/// and as a sub-attribute.
#[test]
fn a_bare_scalar_member_reads_as_a_concise_property() {
    for (member, value) in [(json!(42), "42"), (json!("x"), r#""x""#), (json!(true), "true"), (json!(1.50), "1.5")] {
        assert_eq!(property_value(&single(&member)), &NgsiLdValue::Json(literal(value)), "{member}");
    }
    let instance = single(&json!({"type": "Property", "value": 1, "accuracy": 0.5}));
    let accuracy = instance.common().sub_attributes.get("accuracy").expect("the sub-attribute");
    let AttributeValue::Single(accuracy) = accuracy else {
        panic!("one instance: {accuracy:?}");
    };
    assert_eq!(property_value(accuracy), &NgsiLdValue::Json(literal("0.5")));
}

/// An array none of whose items is an instance is the value of a concise
/// Property, the empty array included.
#[test]
fn an_array_of_values_reads_as_a_concise_property_value() {
    for member in [json!([1, 2, 3]), json!([[1]]), json!([{"a": 1}]), json!([])] {
        let expected = literal(&member.to_string());
        assert_eq!(property_value(&single(&member)), &NgsiLdValue::Json(expected), "{member}");
    }
}

/// An object with no `type` and no value member is a Property value.
#[test]
fn a_typeless_object_without_a_value_member_reads_as_a_property_value() {
    let instance = single(&json!({"street": "Main", "no": 1}));
    assert_eq!(property_value(&instance), &NgsiLdValue::Json(literal(r#"{"street":"Main","no":1}"#)));
}

/// An object with no `type` carrying `value` is a Property, or a
/// `GeoProperty` when the value is a geometry (clauses 4.5.2.3 and 4.7.3).
#[test]
fn a_typeless_object_with_value_reads_as_a_property_or_geo_property() {
    let Attribute::Property(property) = single(&json!({"value": 21, "unitCode": "CEL"})) else {
        panic!("a Property");
    };
    assert_eq!(property.unit_code.map(|code| code.as_str().to_owned()).as_deref(), Some("CEL"));
    let geo = single(&json!({"value": {"type": "Point", "coordinates": [1, 2]}, "accuracy": 3}));
    let Attribute::GeoProperty(geo) = geo else {
        panic!("a GeoProperty: {geo:?}");
    };
    assert!(geo.common.sub_attributes.contains_key("accuracy"));
}

/// An object with no `type` is the family its value member names.
#[test]
fn a_typeless_object_reads_as_the_family_its_value_member_names() {
    for (member, family) in [
        (json!({"object": "urn:ngsi-ld:A:1"}), AttributeType::Relationship),
        (json!({"languageMap": {"en": "a"}}), AttributeType::LanguageProperty),
        (json!({"vocab": "x"}), AttributeType::VocabProperty),
        (json!({"valueList": [1]}), AttributeType::ListProperty),
        (json!({"objectList": ["urn:ngsi-ld:A:1"]}), AttributeType::ListRelationship),
        (json!({"json": {"k": 1}}), AttributeType::JsonProperty),
    ] {
        assert_eq!(single(&member).attribute_type(), family, "{member}");
    }
}

/// A bare geometry is a concise `GeoProperty` (clause 4.7.3).
#[test]
fn a_bare_geometry_reads_as_a_concise_geo_property() {
    let Attribute::GeoProperty(geo) = single(&json!({"type": "Point", "coordinates": [13.4, 52.5]})) else {
        panic!("a GeoProperty");
    };
    assert!(matches!(geo.value, GeoPropertyValue::Geometry(_)));
}

/// An array of instances, some of them concise, is a multi-attribute.
#[test]
fn an_array_of_concise_instances_reads_as_a_multi_attribute() {
    let member = json!([{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}]);
    let Ok(AttributeValue::Multiple(instances)) = read("speed", &member) else {
        panic!("a multi-attribute");
    };
    assert_eq!(instances.len(), 2);
    assert_eq!(
        instances[0].dataset_id().map(|id| id.as_str().to_owned()).as_deref(),
        Some("urn:ngsi-ld:Dataset:a")
    );
}

/// Clause 4.5.5: `"datasetId": "@none"` is the default Attribute instance on
/// every family, so it reads as an instance with no `datasetId`, and beside a
/// named instance it keeps the two instances apart.
#[test]
fn a_dataset_id_none_reads_as_the_default_instance() {
    for member in [
        json!({"type": "Property", "value": 1, "datasetId": "@none"}),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:1", "datasetId": "@none"}),
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}, "datasetId": "@none"}),
        json!({"type": "LanguageProperty", "languageMap": {"fr": "Grand Place"}, "datasetId": "@none"}),
    ] {
        assert!(single(&member).dataset_id().is_none(), "{member}");
    }
    let member = json!([{"type": "Property", "value": 1, "datasetId": "@none"}, {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:a"}]);
    let Ok(AttributeValue::Multiple(instances)) = read("speed", &member) else {
        panic!("a multi-attribute");
    };
    assert_eq!(
        instances
            .iter()
            .map(|instance| instance.dataset_id().map(|id| id.as_str().to_owned()))
            .collect::<Vec<_>>(),
        vec![None, Some("urn:ngsi-ld:Dataset:a".to_owned())]
    );
}

/// Clause 4.6.3: a comma fraction separator is accepted in a request, on
/// `observedAt` and `expiresAt`.
#[test]
fn an_observed_at_with_a_comma_fraction_is_read_as_the_instant_it_names() {
    let attribute = single(&json!({
        "type": "Property", "value": 1,
        "observedAt": "2024-01-01T00:00:00,25Z",
        "expiresAt": "2099-01-01T00:00:00,5Z"
    }));
    let Attribute::Property(property) = attribute else {
        panic!("a Property");
    };
    pretty_assertions::assert_eq!(property.common.observed_at.map(instant_text).as_deref(), Some("2024-01-01T00:00:00.250Z"));
    pretty_assertions::assert_eq!(property.common.expires_at.map(instant_text).as_deref(), Some("2099-01-01T00:00:00.500Z"));
}

/// A geometry written as a JSON string is kept as its text (clause
/// 4.7.2); a string that is no geometry is refused.
#[test]
fn a_string_encoded_geometry_reads_as_a_geo_property() {
    let text = json!({"type": "Point", "coordinates": [1, 2]}).to_string();
    let Attribute::GeoProperty(geo) = single(&json!({"type": "GeoProperty", "value": text})) else {
        panic!("a GeoProperty");
    };
    let GeoPropertyValue::Encoded(encoded) = &geo.value else {
        panic!("the string form: {:?}", geo.value);
    };
    assert_eq!(encoded.as_str(), text);
    assert_eq!(refused(&json!({"type": "GeoProperty", "value": "not json"})).path.to_string(), "/value");
}

/// The NGSI-LD Null in place of the whole member.
#[test]
fn a_bare_null_member_reads_as_the_ngsi_ld_null() {
    assert!(matches!(read("owner", &json!("urn:ngsi-ld:null")), Ok(AttributeValue::NgsiLdNull)));
}

/// A `GeoProperty` whose value is the NGSI-LD Null (clause 4.5.0).
#[test]
fn a_geo_property_null_value_reads_typed() {
    let Attribute::GeoProperty(geo) = single(&json!({"type": "GeoProperty", "value": "urn:ngsi-ld:null"})) else {
        panic!("a GeoProperty");
    };
    assert_eq!(geo.value, GeoPropertyValue::Null);
}

/// Clause 3.1 names the URN alone, so a `GeoProperty` valued with the IRI the
/// `ngsi-ld` prefix makes of `ngsi-ld:null` holds a string that is no
/// geometry.
#[test]
fn a_geo_property_valued_with_the_null_lookalike_iri_is_refused() {
    assert!(read("location", &json!({"type": "GeoProperty", "value": "https://uri.etsi.org/ngsi-ld/null"})).is_err());
}

/// No member admits a JSON `null` directly (clause 5.2.1).
#[test]
fn a_json_null_member_or_family_value_is_refused() {
    assert!(matches!(refused(&Value::Null).refusal, NodeRefusal::Null));
    for (member, at) in [
        (json!({"type": "Property", "value": null}), "/value"),
        (json!({"type": "GeoProperty", "value": null}), "/value"),
        (json!({"type": "Relationship", "object": null}), "/object"),
        (json!({"type": "VocabProperty", "vocab": null}), "/vocab"),
        (json!({"type": "LanguageProperty", "languageMap": null}), "/languageMap"),
        (json!({"type": "ListProperty", "valueList": null}), "/valueList"),
        (json!({"type": "JsonProperty", "json": null}), "/json"),
    ] {
        let refusal = refused(&member);
        assert!(matches!(refusal.refusal, NodeRefusal::Null), "{member}: {refusal}");
        assert_eq!(refusal.path.to_string(), at, "{member}");
    }
}

/// Nodes no clause admits where an attribute stands.
#[test]
fn a_node_no_clause_admits_is_refused_with_its_fault() {
    for (member, fault) in [
        (json!({"type": 7}), AttributeNodeFault::TypeNotAString),
        (
            json!({"type": "Propertyy", "value": 1}),
            AttributeNodeFault::NotAnAttributeType(strum::ParseError::VariantNotFound),
        ),
        (
            json!({"type": "Feature"}),
            AttributeNodeFault::NotAnAttributeType(strum::ParseError::VariantNotFound),
        ),
        (
            json!({"type": "https://uri.etsi.org/ngsi-ld/Property", "value": 1}),
            AttributeNodeFault::NotAnAttributeType(strum::ParseError::VariantNotFound),
        ),
        (json!({"type": "Point"}), AttributeNodeFault::GeometryWithoutCoordinates),
        (json!({"object": "urn:ngsi-ld:A:1", "vocab": "x"}), AttributeNodeFault::SeveralValueMembers),
        (json!([{"type": "Property", "value": 1}, 1]), AttributeNodeFault::MixedInstancesAndValues),
    ] {
        let NodeRefusal::Shape(found) = refused(&member).refusal else {
            panic!("a shape refusal: {member}");
        };
        assert_eq!(found, fault, "{member}");
    }
}

/// An instance missing its family's value member.
#[test]
fn an_instance_without_its_value_member_is_refused() {
    for family in AttributeType::ALL {
        let refusal = refused(&json!({"type": family.to_string()}));
        assert!(matches!(refusal.refusal, NodeRefusal::Missing { .. }), "{family}: {refusal}");
    }
}

/// A family value or an envelope member that does not decode is
/// refused at the member.
#[test]
fn a_malformed_family_value_or_envelope_member_is_refused_where_it_stands() {
    for (member, at) in [
        (json!({"type": "Relationship", "object": "not a uri"}), "/object"),
        (json!({"type": "VocabProperty", "vocab": "bad ref"}), "/vocab"),
        (json!({"type": "LanguageProperty", "languageMap": {"en us": "a"}}), "/languageMap"),
        (json!({"type": "LanguageProperty", "languageMap": "urn:ngsi-ld:null"}), "/languageMap"),
        (json!({"type": "ListProperty", "valueList": 1}), "/valueList"),
        (
            json!({"type": "ListProperty", "valueList": [1, {"type": "Point", "coordinates": [1, 99]}]}),
            "/valueList/1",
        ),
        (
            json!({"type": "ListRelationship", "objectList": [{"subject": "urn:ngsi-ld:A:1"}]}),
            "/objectList",
        ),
        (json!({"type": "Property", "value": {"type": "Point", "coordinates": [200, 0]}}), "/value"),
        (
            json!({"type": "GeoProperty", "value": {"type": "GeometryCollection", "geometries": []}}),
            "/value",
        ),
        (
            json!({"type": "Property", "value": 1, "previousValue": {"type": "Point", "coordinates": [1]}}),
            "/previousValue",
        ),
        (json!({"type": "Property", "value": 1, "unitCode": "QQQ"}), "/unitCode"),
        (json!({"type": "Property", "value": 1, "valueType": "bad ref"}), "/valueType"),
        (json!({"type": "Property", "value": 1, "observedAt": "yesterday"}), "/observedAt"),
        (json!({"type": "Property", "value": 1, "expiresAt": "2030-01-01"}), "/expiresAt"),
        (json!({"type": "Property", "value": 1, "datasetId": "not a uri"}), "/datasetId"),
        (json!({"type": "Property", "value": 1, "instanceId": 3}), "/instanceId"),
    ] {
        assert_eq!(refused(&member).path.to_string(), at, "{member}");
    }
}

/// Table 5.2.5-1 and clauses 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.22.2 and 4.5.24.2:
/// a `unitCode` that is no common code, and one on a unitless family, is refused
/// at the member in words naming the attribute, the family and `'unitCode'`;
/// the three quantity families keep theirs as their own member.
#[test]
fn a_unit_code_is_read_on_the_quantity_families_and_refused_elsewhere() {
    let unusable = refused(&json!({"type": "Property", "value": 1, "unitCode": "km/h"}));
    assert_eq!(unusable.path.to_string(), "/unitCode");
    assert_eq!(unusable.refusal.to_string(), "attribute 'member': 'unitCode' is not a UN/CEFACT common code");
    for family in AttributeType::ALL {
        let member = match family {
            AttributeType::Property => json!({"type": "Property", "value": 1, "unitCode": "KMH"}),
            AttributeType::GeoProperty => json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}, "unitCode": "KMH"}),
            AttributeType::ListProperty => json!({"type": "ListProperty", "valueList": [1], "unitCode": "KMH"}),
            AttributeType::Relationship => json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1", "unitCode": "KMH"}),
            AttributeType::LanguageProperty => json!({"type": "LanguageProperty", "languageMap": {"en": "a"}, "unitCode": "KMH"}),
            AttributeType::VocabProperty => json!({"type": "VocabProperty", "vocab": "x", "unitCode": "KMH"}),
            AttributeType::JsonProperty => json!({"type": "JsonProperty", "json": {"a": 1}, "unitCode": "KMH"}),
            AttributeType::ListRelationship => json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1"], "unitCode": "KMH"}),
        };
        match family {
            AttributeType::Property | AttributeType::GeoProperty | AttributeType::ListProperty => {
                let instance = single(&member);
                assert!(instance.common().sub_attributes.is_empty(), "{family}: {instance:?}");
            }
            AttributeType::Relationship
            | AttributeType::LanguageProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty
            | AttributeType::ListRelationship => {
                assert_eq!(
                    refused(&member).refusal.to_string(),
                    format!("attribute 'member': a {family} carries no 'unitCode'"),
                    "{family}"
                );
            }
        }
    }
}

/// A system member a Context Producer wrote is ignored, whatever it holds
/// (clause 5.2.2); a peer's malformed one is refused.
#[test]
fn a_system_member_a_producer_writes_is_ignored() {
    let text = json!({
        "id": "urn:ngsi-ld:T:1",
        "type": "T",
        "createdAt": true,
        "speed": {"type": "Property", "value": 1, "createdAt": true, "modifiedAt": "x"}
    })
    .to_string();
    let entity = <Entity>::from_request_text(text.as_bytes(), CoreTermAliases::keyword_spellings()).expect("the system members are ignored");
    assert!(entity.created_at.is_none());
    let speed = entity.attrs.get("speed").and_then(|speed| speed.instances().next()).expect("the attribute");
    assert!(speed.common().created_at.is_none() && speed.common().modified_at.is_none());
    assert!(serde_json::from_str::<Entity>(&text).is_err(), "a peer's malformed system member is refused");
}

/// A sub-attribute named by no attribute name is refused at its name.
#[test]
fn a_sub_attribute_named_by_no_attribute_name_is_refused() {
    let refusal = refused(&json!({"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 1}}));
    assert!(matches!(refusal.refusal, NodeRefusal::AttributeName(_)), "{refusal}");
    assert_eq!(refusal.path.to_string(), "/bad name");
}

/// A geometry carrying a JSON-LD keyword is no `GeoJSON` geometry.
#[test]
fn a_geometry_carrying_a_json_ld_keyword_is_refused() {
    let refusal = refused(&json!({"type": "GeoProperty", "value": {"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}}));
    assert!(matches!(refusal.refusal, NodeRefusal::Geometry { .. }), "{refusal}");
}

/// A refusal is located from the top of the member, through sub-attributes
/// and the elements of a multi-attribute.
#[test]
fn a_refusal_names_where_it_stands() {
    let member = json!([
        {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"},
        {"type": "Property", "value": 2, "accuracy": {"type": "Propertyy", "value": 0.5}}
    ]);
    assert_eq!(refused(&member).path.to_string(), "/1/accuracy");
}

/// Clause 4.5.2.2: the seals of the `ngsildproof` Property are its own
/// non-reified members; anywhere else they "shall never be present" and are
/// refused where they stand, not read as sub-attributes of that name.
#[test]
fn the_seals_of_ngsildproof_are_members_of_the_property() {
    let proof = json!({"type": "Property", "value": {"proofValue": "z"}, "entityIdSealed": "urn:ngsi-ld:Store:2", "entityTypeSealed": "Store"});
    let Ok(AttributeValue::Single(instance)) = read("ngsildproof", &proof) else {
        panic!("one instance");
    };
    let Attribute::Property(property) = *instance else {
        panic!("a Property");
    };
    assert_eq!(property.entity_id_sealed, Some(literal(r#""urn:ngsi-ld:Store:2""#)));
    assert_eq!(property.entity_type_sealed, Some(literal(r#""Store""#)));
    assert!(property.common.sub_attributes.is_empty());
    assert_eq!(rendered(&AttributeValue::Single(Box::new(Attribute::Property(property)))), proof);
    for (name, member, at) in [
        ("member", proof, "/"),
        (
            "ngsildproof",
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:x:1"}),
            "/",
        ),
        ("ngsildproof", json!({"object": "urn:ngsi-ld:Proof:1", "entityTypeSealed": "T"}), "/"),
        (
            "speed",
            json!({"type": "Property", "value": 1, "accuracy": {"value": 0.5, "entityIdSealed": "urn:x:1"}}),
            "/accuracy",
        ),
    ] {
        let Err(NodeReadError::Refused(refusal)) = read(name, &member) else {
            panic!("a seal outside ngsildproof is refused: {member}");
        };
        assert!(
            matches!(refusal.refusal, NodeRefusal::Shape(AttributeNodeFault::ProofSealOutsideNgsildproof(_))),
            "{member}: {refusal}"
        );
        assert_eq!(refusal.path.to_string(), at, "{member}");
    }
}

/// A Property that carries every member it may carry reads back as written.
#[test]
fn every_family_with_every_member_renders_as_it_was_written() {
    for instance in [
        json!({"type": "Property", "value": 1.5, "unitCode": "CEL", "valueType": "Number", "previousValue": 2, "datasetId": "urn:ngsi-ld:Dataset:a",
            "observedAt": "2024-01-01T00:00:00Z", "instanceId": "urn:ngsi-ld:attribute:instance:1",
            "accuracy": {"type": "Property", "value": 0.5}}),
        json!({"type": "Relationship", "object": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"], "objectType": ["A", "ex:B"], "previousObject": "urn:ngsi-ld:A:0",
            "entity": {"id": "urn:ngsi-ld:A:1", "type": "A", "name": {"type": "Property", "value": "a"}}}),
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "valueType": "geojson:Point"}),
        json!({"type": "JsonProperty", "json": {"id": "x", "n": [1, 2.25]}, "previousJson": {"a": 1}, "valueType": "json"}),
        json!({"type": "ListProperty", "valueList": [1, "a", [3]], "previousValueList": [0], "valueType": "Integer"}),
    ] {
        assert_eq!(rendered(&AttributeValue::Single(Box::new(single(&instance)))), instance);
    }
}

/// A Property carrying a chain of `levels` sub-attributes, each a Property
/// of a scalar.
fn sub_attribute_chain(levels: usize) -> Value {
    (0..levels).fold(
        json!({"type": "Property", "value": 1}),
        |inner, _| json!({"type": "Property", "value": 1, "s": inner}),
    )
}

/// How many sub-attributes `member` nests.
fn chain_length(member: &AttributeValue) -> usize {
    let AttributeValue::Single(instance) = member else {
        panic!("one instance: {member:?}");
    };
    instance.common().sub_attributes.get("s").map_or(0, |inner| 1 + chain_length(inner))
}

/// The reading recurses once per sub-attribute, and every level runs on
/// sufficient stack: a chain as long as the reader admits in one attribute is
/// read from request text on a 512 KiB thread.
#[test]
fn a_chain_of_sub_attributes_as_long_as_the_reader_admits_is_read_on_a_small_stack() {
    let text = json!({"id": "urn:ngsi-ld:Probe:1", "type": "Probe", "p": sub_attribute_chain(124)}).to_string();
    let length = thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let entity = <Entity>::from_request_text(text.as_bytes(), CoreTermAliases::keyword_spellings()).expect("the entity is read");
            chain_length(entity.attrs.get("p").expect("the attribute is read"))
        })
        .expect("the thread starts")
        .join()
        .expect("the reading stays within the stack");
    assert_eq!(length, 124);
}

/// An entity that is none is refused, located at the member at fault.
#[test]
fn an_entity_that_is_none_is_refused() {
    for (refused_entity, at) in [
        (json!({"type": "T"}), "/"),
        (json!({"id": "urn:ngsi-ld:T:1"}), "/"),
        (json!({"id": "not a uri", "type": "T"}), "/id"),
        (json!({"id": "urn:ngsi-ld:T:1", "type": "T", "bad name": 1}), "/bad name"),
        (json!({"id": "urn:ngsi-ld:T:1", "type": "T", "scope": "no scope"}), "/scope"),
        (json!({"id": "urn:ngsi-ld:T:1", "type": "T", "speed": {"type": "Property"}}), "/speed"),
    ] {
        let text = RawValue::from_string(refused_entity.to_string()).expect("the fixture is JSON");
        let Err(NodeReadError::Refused(refusal)) = <Entity>::from_node(JsonPiece::new(&text)) else {
            panic!("refused: {refused_entity}");
        };
        assert_eq!(refusal.path.to_string(), at, "{refused_entity}");
    }
    let array = RawValue::from_string(json!([{"id": "urn:ngsi-ld:T:1", "type": "T"}]).to_string()).expect("the fixture is JSON");
    assert!(<Entity>::from_node(JsonPiece::new(&array)).is_err());
}

/// Clause 4.5.2.3: the empty array is the value of a concise Property, at the
/// top level of an entity as at any other place.
#[test]
fn an_empty_array_member_reads_as_an_empty_property_value() {
    assert_eq!(property_value(&single(&json!([]))), &NgsiLdValue::Json(literal("[]")));
    let entity = <Entity>::from_request_text(br#"{"id": "urn:ngsi-ld:T:1", "type": "T", "tags": []}"#, CoreTermAliases::keyword_spellings())
        .expect("the entity is read");
    let Some(AttributeValue::Single(tags)) = entity.attrs.get("tags") else {
        panic!("one instance: {:?}", entity.attrs);
    };
    assert_eq!(property_value(tags), &NgsiLdValue::Json(literal("[]")));
}

/// The instance `member` reads as under the name `member`, read through
/// `aliases`.
fn single_through(member: &Value, aliases: &CoreTermAliases) -> Attribute {
    let text = RawValue::from_string(member.to_string()).expect("the member is JSON");
    match <AttributeValue as MemberReading>::from_node("member", AliasedNode::new(JsonPiece::new(&text), aliases)) {
        Ok(AttributeValue::Single(instance)) => *instance,
        other => panic!("one instance: {other:?} from {member}"),
    }
}

/// Clause 4.4: `@type` stands for `type`, on an instance and on its
/// sub-attributes.
#[test]
fn a_keyword_spelled_instance_reads_typed() {
    let instance = single_through(
        &json!({"@type": "Relationship", "object": "urn:ngsi-ld:T:2", "since": {"@type": "Property", "value": 1}}),
        CoreTermAliases::keyword_spellings(),
    );
    assert!(matches!(instance, Attribute::Relationship(_)), "{instance:?}");
    let Some(AttributeValue::Single(since)) = instance.common().sub_attributes.get("since") else {
        panic!("one sub-attribute instance: {instance:?}");
    };
    assert_eq!(property_value(since), &NgsiLdValue::Json(literal("1")));
}

/// Clause 5.5.7: the terms a context defines for `@type`, `ngsi-ld:hasValue`
/// and a core class are read as those core terms.
#[test]
fn an_aliased_instance_reads_typed() {
    let mut aliases = CoreTermAliases::keyword_spellings().clone(); // clone: the fixture extends the shared table
    aliases.alias("kind", "type");
    aliases.alias("v", "value");
    aliases.alias("Prop", "Property");
    let instance = single_through(&json!({"kind": "Prop", "v": 21, "unitCode": "CEL"}), &aliases);
    assert_eq!(property_value(&instance), &NgsiLdValue::Json(literal("21")));
    let Attribute::Property(property) = &instance else {
        panic!("a Property: {instance:?}");
    };
    assert!(property.unit_code.is_some(), "{instance:?}");
}
