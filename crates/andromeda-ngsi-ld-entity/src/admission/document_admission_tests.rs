//! The admission rules of a written entity document and of a bare attribute fragment.

use crate::{
    admission::{instance_completeness::InstanceCompleteness, null_sentinel_admission::NullSentinelAdmission},
    document::{EntityDocument, instance_document::InstanceDocument},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier_length::IDENTIFIER_BYTE_LIMIT};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Value, json};

/// Every admission, so a case can state what each one does with a payload.
const ADMISSIONS: [NullSentinelAdmission; 4] = [
    NullSentinelAdmission::Refused,
    NullSentinelAdmission::PartialUpdateFragment,
    NullSentinelAdmission::MergeFragment,
    NullSentinelAdmission::TemporalEvolution,
];

/// Every admission but the merge's, which alone is exempt from the third
/// prohibition of clause 5.5.4.
const OUTSIDE_A_MERGE: [NullSentinelAdmission; 3] = [
    NullSentinelAdmission::Refused,
    NullSentinelAdmission::PartialUpdateFragment,
    NullSentinelAdmission::TemporalEvolution,
];

/// One expanded entity carrying the attribute member `member`.
fn entity_with(member: &Value) -> Value {
    json!([{ "@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"], "https://example.org/member": member }])
}

/// One expanded entity carrying a single instance of `member`.
fn entity_with_instance(instance: &Value) -> Value {
    entity_with(&json!([instance]))
}

/// Decode `document` and admit it under `admission`.
fn admit(document: &Value, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
    let bytes = serde_json::to_vec(document).expect("JSON");
    EntityDocument::from_slice(&bytes)?.admit(admission)
}

/// Decode the attribute fragment `fragment` and admit it under `admission`,
/// as a fragment patching the members it names.
fn admit_fragment(fragment: &Value, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
    admit_fragment_as(fragment, admission, InstanceCompleteness::Partial)
}

/// Decode the attribute fragment `fragment` and admit it under `admission`, as
/// an instance of the `completeness` the operation writing it requires.
fn admit_fragment_as(fragment: &Value, admission: NullSentinelAdmission, completeness: InstanceCompleteness) -> Result<(), EntityRefusal> {
    let attribute: AttributeName = "https://example.org/member".parse().expect("a name");
    let bytes = serde_json::to_vec(fragment).expect("JSON");
    InstanceDocument::from_slice(&attribute, &bytes)?.admit_fragment(&attribute, admission, completeness)
}

/// The value member each family carries, in its expanded shape.
fn own_value(family: AttributeType) -> Value {
    match family {
        AttributeType::Property => json!([{"@value": "kept"}]),
        AttributeType::GeoProperty => json!([{"@value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}]),
        AttributeType::Relationship | AttributeType::VocabProperty => json!([{"@id": "urn:ngsi-ld:Device:target"}]),
        AttributeType::LanguageProperty => json!([{"@value": "kept", "@language": "en"}]),
        AttributeType::JsonProperty => json!([{"@type": "@json", "@value": {"kept": true}}]),
        AttributeType::ListProperty => json!([{"@list": [{"@value": "kept"}]}]),
        AttributeType::ListRelationship => json!([{"@list": [{"@id": "urn:ngsi-ld:Device:target"}]}]),
    }
}

/// Clauses 4.5.3, 4.5.18, 4.5.20 and 4.5.24 define these families without a
/// `unitCode`, so one on them is invalid input rather than a value to store.
#[test]
fn a_unit_code_on_a_family_the_specification_leaves_unitless_is_refused() {
    for family in [
        AttributeType::Relationship,
        AttributeType::LanguageProperty,
        AttributeType::VocabProperty,
        AttributeType::JsonProperty,
        AttributeType::ListRelationship,
    ] {
        let instance =
            json!({"@type": [family.iri().as_str()], family.value_key_str(): own_value(family), properties::UNIT_CODE.as_str(): [{"@value": "KGM"}]});
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("the family admits no unitCode");
        assert!(error.to_string().contains("unitCode"), "{family}: {error}");
        assert!(error.to_string().contains("defined without one"), "{family}: {error}");
    }
}

#[test]
fn a_property_still_carries_its_unit_code() {
    let instance = json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 1}], properties::UNIT_CODE.as_str(): [{"@value": "KGM"}]});
    admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect("a Property defines a unitCode member");
}

/// Table 5.2.5-1 restricts `unitCode` to a UN/CEFACT common code.
#[test]
fn a_property_carrying_a_unit_that_is_no_common_code_is_refused() {
    let instance = json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 1}], properties::UNIT_CODE.as_str(): [{"@value": "Celsius"}]});
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("Celsius is no common code");
    pretty_assertions::assert_eq!(
        error.to_string(),
        "'https://example.org/member' carries an unusable 'unitCode': 'unitCode' is not a UN/CEFACT common code"
    );
}

/// A unit-code deletion is no unit and is not held to the common-code list.
#[test]
fn a_unit_code_deletion_is_not_held_to_the_common_code_list() {
    for sentinel in [json!({"@value": "urn:ngsi-ld:null"}), json!({"@id": "urn:ngsi-ld:null"})] {
        let instance =
            json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 1}], properties::UNIT_CODE.as_str(): [sentinel]});
        admit(&entity_with_instance(&instance), NullSentinelAdmission::PartialUpdateFragment).expect("a deletion names no unit");
    }
}

#[test]
fn a_relationship_without_a_unit_code_is_accepted() {
    let instance = json!({"@type": [AttributeType::Relationship.iri().as_str()], properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}]});
    admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect("a Relationship with no unit is well formed");
}

/// Clause 4.5.5.1 admits exactly one default instance per attribute name.
#[test]
fn two_default_instances_of_one_attribute_are_refused() {
    let property = |value: i64| json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": value}]});
    let error = admit(&entity_with(&json!([property(1), property(9)])), NullSentinelAdmission::Refused).expect_err("one default instance per attribute name");
    assert!(error.to_string().contains("default instance"), "{error}");
}

/// Two instances of one `datasetId` name the same instance twice.
#[test]
fn two_instances_sharing_a_dataset_id_are_refused() {
    let property = |value: i64| json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": value}], properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:a"}]});
    let error = admit(&entity_with(&json!([property(1), property(9)])), NullSentinelAdmission::Refused).expect_err("one instance per datasetId");
    assert!(error.to_string().contains("urn:ngsi-ld:Dataset:a"), "{error}");
}

/// Clause 4.5.5 identifies an instance by its `datasetId` and clause 4.5.7
/// keeps a client's `instanceId` as given; both are held to the identifier
/// bound, and the refusal names the member and the bound.
#[test]
fn a_dataset_id_or_instance_id_one_octet_past_the_identifier_bound_is_refused_by_name() {
    let instance = |member: &str, prefix: &str, length: usize| {
        json!({
            "@type": [AttributeType::Property.iri().as_str()],
            properties::VALUE.as_str(): [{"@value": 1}],
            member: [{"@id": format!("{prefix}{}", "x".repeat(length - prefix.len()))}],
        })
    };
    for (member, name, prefix) in [
        (properties::DATASET_ID.as_str(), "datasetId", "urn:ngsi-ld:Dataset:"),
        (properties::INSTANCE_ID.as_str(), "instanceId", "urn:ngsi-ld:Instance:"),
    ] {
        admit(
            &entity_with_instance(&instance(member, prefix, IDENTIFIER_BYTE_LIMIT)),
            NullSentinelAdmission::Refused,
        )
        .expect("an identity at the bound");
        let error = admit(
            &entity_with_instance(&instance(member, prefix, IDENTIFIER_BYTE_LIMIT + 1)),
            NullSentinelAdmission::Refused,
        )
        .expect_err("an identity one octet past the bound");
        let message = error.to_string();
        assert!(
            message.contains(name) && message.contains("1025 bytes") && message.contains("1024"),
            "{message}"
        );
    }
}

#[test]
fn a_default_instance_beside_named_ones_is_accepted() {
    let property = |dataset_id: Option<&str>| {
        let mut instance = json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 1}]});
        if let Some(dataset_id) = dataset_id {
            instance[properties::DATASET_ID.as_str()] = json!([{"@id": dataset_id}]);
        }
        instance
    };
    let member = json!([property(None), property(Some("urn:ngsi-ld:Dataset:a")), property(Some("urn:ngsi-ld:Dataset:b"))]);
    admit(&entity_with(&member), NullSentinelAdmission::Refused).expect("distinct instance identities are well formed");
}

/// Clause 4.18 lets one entity declare several scope paths: one member, not
/// several instances.
#[test]
fn an_entity_declaring_several_scope_paths_is_accepted() {
    let document = json!([{"@id": "urn:ngsi-ld:Building:1", properties::SCOPE.as_str(): [{"@value": "/Madrid/Gardens"}, {"@value": "/CompanyA/UnitC"}]}]);
    admit(&document, NullSentinelAdmission::Refused).expect("several scope paths are one member, not two instances");
}

/// Clause 4.5.18: an attribute fragment carrying two languages is one
/// instance, not two.
#[test]
fn an_attribute_fragment_declaring_several_languages_is_accepted() {
    let fragment = json!([{properties::LANGUAGE_MAP.as_str(): [{"@value": "Grand Place", "@language": "en"}, {"@value": "Gran Lugar", "@language": "es"}]}]);
    admit_fragment(&fragment, NullSentinelAdmission::PartialUpdateFragment).expect("two languages are one languageMap, not two instances");
}

/// Clause 4.5.3.2: a Relationship "shall never include" `value`.
#[test]
fn a_relationship_carrying_a_property_value_is_refused() {
    let instance = json!({"@type": [AttributeType::Relationship.iri().as_str()], properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}], properties::VALUE.as_str(): [{"@value": 1}]});
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("clause 4.5.3.2 prohibits value on a Relationship");
    assert!(error.to_string().contains("value"), "{error}");
}

/// Clause 4.5.2.2: a Property "shall never include" another family's value
/// member, nor the output-only spellings.
#[test]
fn a_property_carrying_any_other_familys_value_member_is_refused() {
    for member in [
        properties::OBJECT,
        properties::OBJECT_LIST,
        properties::LANGUAGE_MAP,
        properties::VOCAB,
        properties::JSON,
        properties::VALUE_LIST,
        properties::ENTITY,
        properties::ENTITY_LIST,
        properties::PREVIOUS_OBJECT,
        properties::PREVIOUS_OBJECT_LIST,
        properties::PREVIOUS_LANGUAGE_MAP,
        properties::PREVIOUS_VOCAB,
        properties::PREVIOUS_JSON,
        properties::PREVIOUS_VALUE_LIST,
    ] {
        let instance =
            json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 1}], member.as_str(): [{"@value": "forged"}]});
        admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err(&format!("clause 4.5.2.2 prohibits {member} on a Property"));
    }
}

/// Clauses 4.5.18.2, 4.5.20.2, 4.5.21.2, 4.5.24.2: `value` generalizes these
/// families' own member.
#[test]
fn the_specialized_property_families_refuse_the_generalized_value_member() {
    for family in [
        AttributeType::LanguageProperty,
        AttributeType::VocabProperty,
        AttributeType::JsonProperty,
        AttributeType::ListProperty,
    ] {
        let instance = json!({"@type": [family.iri().as_str()], family.value_key_str(): own_value(family), properties::VALUE.as_str(): [{"@value": 1}]});
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("value generalizes this family's own member");
        assert!(error.to_string().contains("value"), "{family}: {error}");
    }
}

/// Clause 4.5.22.2: `object` generalizes `objectList`.
#[test]
fn a_list_relationship_carrying_the_generalized_object_is_refused() {
    let instance = json!({
        "@type": [AttributeType::ListRelationship.iri().as_str()],
        properties::OBJECT_LIST.as_str(): [{"@list": [{"@id": "urn:ngsi-ld:Device:a"}]}],
        properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:b"}],
    });
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("clause 4.5.22.2 prohibits object on a ListRelationship");
    assert!(error.to_string().contains("object"), "{error}");
}

/// A sub-attribute is an attribute (clause 4.5.2.2), so the prohibition
/// reaches it, and the refusal names it.
#[test]
fn a_sub_attribute_carrying_another_familys_value_member_is_refused() {
    let instance = json!({
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [{"@value": 1}],
        "https://example.org/accuracy": [{
            "@type": [AttributeType::Relationship.iri().as_str()],
            properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
            properties::LANGUAGE_MAP.as_str(): [{"@value": "forged", "@language": "en"}],
        }],
    });
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("clause 4.5.3.2 prohibits languageMap on a Relationship");
    assert!(error.to_string().contains("accuracy"), "the offending sub-attribute is named: {error}");
}

/// `location` is a core term and still an attribute.
#[test]
fn an_attribute_named_by_a_core_term_is_still_walked_as_an_attribute() {
    let document = json!([{
        "@id": "urn:ngsi-ld:Device:1",
        properties::LOCATION.as_str(): [{
            "@type": [AttributeType::GeoProperty.iri().as_str()],
            properties::VALUE.as_str(): [{"@value": "{\"type\":\"Point\",\"coordinates\":[1.0,2.0]}"}],
            properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
        }],
    }]);
    let error = admit(&document, NullSentinelAdmission::Refused).expect_err("clause 4.5.2.2 prohibits object on a GeoProperty");
    assert!(error.to_string().contains("object"), "{error}");
}

#[test]
fn every_family_still_accepts_its_own_value_member() {
    for family in AttributeType::ALL {
        let instance = json!({"@type": [family.iri().as_str()], family.value_key_str(): own_value(family)});
        admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect("a family declares its own value member");
    }
}

/// Clause 5.2.5 requires a Property's value, except in a merge fragment,
/// which clause 5.5.12 merges "up to an arbitrary depth" into the instance it
/// targets.
#[test]
fn a_property_missing_its_value_is_refused_outside_a_merge_fragment() {
    let instance = json!({"@type": [AttributeType::Property.iri().as_str()]});
    for admission in [NullSentinelAdmission::Refused, NullSentinelAdmission::PartialUpdateFragment] {
        let error = admit(&entity_with_instance(&instance), admission).expect_err("clause 5.2.5 requires a value");
        assert!(error.to_string().contains("value"), "{admission:?}: {error}");
    }
    admit(&entity_with_instance(&instance), NullSentinelAdmission::MergeFragment).expect("a merge fragment may leave the value out");
}

#[test]
fn a_property_value_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
    let member = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]);
    let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a null value here");
    assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    for admission in [NullSentinelAdmission::PartialUpdateFragment, NullSentinelAdmission::MergeFragment] {
        admit(&entity_with(&member), admission).expect("a fragment deletes a member with the sentinel");
    }
}

#[test]
fn a_relationship_object_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
    let member = json!([{"@type": [AttributeType::Relationship.iri().as_str()], properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]}]);
    let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a null object here");
    assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    admit(&entity_with(&member), NullSentinelAdmission::PartialUpdateFragment).expect("a fragment deletes a Relationship with the sentinel");
}

#[test]
fn a_bare_first_level_null_member_is_refused_only_outside_a_fragment() {
    let member = json!([{"@value": "urn:ngsi-ld:null"}]);
    let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("a bare null member is forbidden");
    assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    admit(&entity_with(&member), NullSentinelAdmission::MergeFragment).expect("a merge fragment deletes the whole member with a bare sentinel");
}

/// Clauses 4.5.21.2 and 4.5.22.2: the list families spell their deletion as
/// a list holding one single NGSI-LD Null.
#[test]
fn a_list_family_deletion_sentinel_is_refused_only_outside_a_fragment() {
    for (family, carrier) in [
        (AttributeType::ListProperty, properties::VALUE_LIST.as_str()),
        (AttributeType::ListRelationship, properties::OBJECT_LIST.as_str()),
    ] {
        let member = json!([{"@type": [family.iri().as_str()], carrier: [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}]);
        let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a list deletion outside a fragment");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{family}: {error}");
        admit(&entity_with(&member), NullSentinelAdmission::PartialUpdateFragment).expect("a fragment deletes a list family with the sentinel");
    }
}

/// Clause 5.5.4: the nested prohibition is excepted for merge fragments alone.
#[test]
fn a_null_nested_in_a_property_value_object_is_refused_outside_a_merge() {
    let member = json!([{
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [{"https://example.org/street": [{"@value": "Pariser Platz"}], "https://example.org/country": [{"@value": "urn:ngsi-ld:null"}]}]
    }]);
    for admission in [NullSentinelAdmission::Refused, NullSentinelAdmission::PartialUpdateFragment] {
        let error = admit(&entity_with(&member), admission).expect_err("only a merge fragment may nest the sentinel in a value object");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{admission:?}: {error}");
    }
    admit(&entity_with(&member), NullSentinelAdmission::MergeFragment).expect("clause 5.5.12 EXAMPLE 2 deletes a value-object key with the sentinel");
}

/// Clause 5.5.4 third prohibition on a `JsonProperty`, whose `json` is the
/// object on the right-hand side of its value.
#[test]
fn a_null_under_a_key_of_a_json_property_is_refused_outside_a_merge() {
    let member =
        json!([{"@type": [AttributeType::JsonProperty.iri().as_str()], properties::JSON.as_str(): [{"@type": "@json", "@value": {"a": "urn:ngsi-ld:null"}}]}]);
    admit(&entity_with(&member), NullSentinelAdmission::PartialUpdateFragment).expect_err("a partial update is bound by the nested prohibition");
    admit(&entity_with(&member), NullSentinelAdmission::MergeFragment).expect("a merge deletes the key");
}

/// A `JsonProperty` whose `json` is the JSON literal `literal`.
fn json_property(literal: &Value) -> Value {
    json!([{"@type": [AttributeType::JsonProperty.iri().as_str()], properties::JSON.as_str(): [{"@type": "@json", "@value": literal}]}])
}

/// Clause 4.5.24.2: the `json` of a `JsonProperty` is "a raw JSON object (or
/// array of objects)", under every admission and on an attribute fragment too.
#[test]
fn a_json_property_holding_no_object_is_refused() {
    for literal in [
        json!(42),
        json!("x"),
        json!(true),
        json!(null),
        json!([1, 2]),
        json!([{"a": 1}, "b"]),
        json!([[{"a": 1}]]),
    ] {
        for admission in ADMISSIONS {
            let error = admit(&entity_with(&json_property(&literal)), admission).expect_err("no object is no JsonProperty value");
            assert!(matches!(error, EntityRefusal::JsonNotAnObject { .. }), "{literal} {admission:?}: {error:?}");
        }
        let fragment = &json_property(&literal)[0];
        let error = admit_fragment(fragment, NullSentinelAdmission::PartialUpdateFragment).expect_err("no object is no JsonProperty value");
        assert!(matches!(error, EntityRefusal::JsonNotAnObject { .. }), "{literal}: {error:?}");
    }
}

/// Clause 4.5.24.2: an object, an array of objects and the empty array of
/// none are `JsonProperty` values.
#[test]
fn a_json_property_holding_an_object_or_objects_is_accepted() {
    for literal in [json!({"a": [1, "x", null]}), json!([{"a": 1}, {}]), json!([])] {
        for admission in ADMISSIONS {
            admit(&entity_with(&json_property(&literal)), admission).expect("an object or an array of objects");
        }
    }
}

/// A `JsonProperty` sub-attribute is held to clause 4.5.24.2 like an
/// attribute.
#[test]
fn a_json_property_sub_attribute_holding_no_object_is_refused() {
    let member = json!([{
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [{"@value": 1}],
        "https://example.org/settings": json_property(&json!("x")),
    }]);
    let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("the sub-attribute holds no object");
    assert!(matches!(error, EntityRefusal::JsonNotAnObject { .. }), "{error:?}");
}

/// The NGSI-LD Null deleting a `JsonProperty` value is a deletion rather than
/// a value, held to clause 5.5.4 alone.
#[test]
fn the_json_property_deletion_is_no_shape_refusal() {
    let deleted = json_property(&json!("urn:ngsi-ld:null"));
    admit(&entity_with(&deleted), NullSentinelAdmission::PartialUpdateFragment).expect("a fragment deletes the value");
    let error = admit(&entity_with(&deleted), NullSentinelAdmission::Refused).expect_err("a create cannot delete");
    assert!(matches!(error, EntityRefusal::ReservedNullValue { .. }), "{error:?}");
}

/// A Property whose value is the JSON literal `literal`.
fn json_literal_property(literal: &Value) -> Value {
    json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@type": "@json", "@value": literal}]}])
}

/// Assert `document` is refused for the third prohibition of clause 5.5.4
/// outside a merge, and admitted by a merge fragment.
fn assert_nested_null_refused_outside_a_merge(document: &Value) {
    for admission in OUTSIDE_A_MERGE {
        let error = admit(document, admission).expect_err("only a merge fragment may nest the sentinel under a key");
        assert!(matches!(error, EntityRefusal::NestedNullValue { .. }), "{admission:?}: {error:?}");
    }
    admit(document, NullSentinelAdmission::MergeFragment).expect("a merge deletes the key (clause 5.5.12, IETF RFC 7396)");
}

/// Clause 5.5.4 third prohibition on a JSON literal written as the value of a
/// Property (`{"@type": "@json", "@value": ..}`): the literal is the JSON
/// object on the right-hand side of the value.
#[test]
fn a_null_under_a_key_of_a_json_literal_property_value_is_refused_outside_a_merge() {
    assert_nested_null_refused_outside_a_merge(&entity_with(&json_literal_property(&json!({"a": 1, "b": "urn:ngsi-ld:null"}))));
}

/// The same for a JSON literal a context coerces a key of the value object to,
/// at any depth of that literal.
#[test]
fn a_null_under_a_key_of_a_json_literal_inside_a_value_object_is_refused_outside_a_merge() {
    let member = json!([{
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [{"https://example.org/settings": [{"@type": "@json", "@value": {"k": {"b": "urn:ngsi-ld:null"}}}]}]
    }]);
    assert_nested_null_refused_outside_a_merge(&entity_with(&member));
}

/// Only the value of a key is the third prohibition: the sentinel as an item
/// of a bare JSON array, and a JSON `null` under a key, are data.
#[test]
fn a_json_literal_holding_the_null_only_as_a_bare_array_item_or_beside_a_json_null_is_data() {
    for literal in [json!(["urn:ngsi-ld:null"]), json!({"a": null})] {
        for admission in ADMISSIONS {
            admit(&entity_with(&json_literal_property(&literal)), admission).expect("no key holds the sentinel");
        }
    }
}

/// Clause 5.5.4 excepts the temporal evolution from the second prohibition
/// only: a deleted instance is recorded, the sentinel under a key is refused.
#[test]
fn a_temporal_evolution_admits_a_deleted_instance_and_refuses_a_nested_null() {
    let deleted = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]);
    admit(&entity_with(&deleted), NullSentinelAdmission::TemporalEvolution).expect("the temporal evolution represents a deletion so");
    let nested = [
        json!([{
            "@type": [AttributeType::Property.iri().as_str()],
            properties::VALUE.as_str(): [{"https://example.org/country": [{"@value": "urn:ngsi-ld:null"}]}]
        }]),
        json!([{"@type": [AttributeType::JsonProperty.iri().as_str()], properties::JSON.as_str(): [{"@type": "@json", "@value": {"a": "urn:ngsi-ld:null"}}]}]),
    ];
    for member in nested {
        let error = admit(&entity_with(&member), NullSentinelAdmission::TemporalEvolution).expect_err("no temporal exception to the third prohibition");
        assert!(matches!(error, EntityRefusal::NestedNullValue { .. }), "{member}: {error:?}");
    }
}

/// Clauses 5.6.4 and 5.6.19 take a bare attribute fragment: a JSON literal
/// holding the sentinel under a key is refused by both.
#[test]
fn an_attribute_fragment_json_literal_holding_a_null_under_a_key_is_refused_outside_a_merge() {
    let fragment = json_literal_property(&json!({"b": "urn:ngsi-ld:null"}));
    for admission in [NullSentinelAdmission::Refused, NullSentinelAdmission::PartialUpdateFragment] {
        let error = admit_fragment(&fragment, admission).expect_err("only a merge fragment may nest the sentinel under a key");
        assert!(matches!(error, EntityRefusal::NestedNullValue { .. }), "{admission:?}: {error:?}");
    }
}

/// The Null placements run on their own without the envelope rules: an
/// instance without the value its family requires is no Null placement.
#[test]
fn the_null_placements_alone_skip_the_envelope_rules() {
    let valueless = entity_with_instance(&json!({"@type": [AttributeType::Property.iri().as_str()]}));
    let document = EntityDocument::from_slice(&serde_json::to_vec(&valueless).expect("JSON")).expect("a document");
    document
        .admit_null_placements(NullSentinelAdmission::TemporalEvolution)
        .expect("no NGSI-LD Null stands in the document");
    let nested = entity_with(&json_literal_property(&json!({"b": "urn:ngsi-ld:null"})));
    let document = EntityDocument::from_slice(&serde_json::to_vec(&nested).expect("JSON")).expect("a document");
    let error = document
        .admit_null_placements(NullSentinelAdmission::TemporalEvolution)
        .expect_err("the third prohibition");
    assert!(matches!(error, EntityRefusal::NestedNullValue { .. }), "{error:?}");
    let attribute: AttributeName = "https://example.org/member".parse().expect("a name");
    let fragment = json_literal_property(&json!({"b": "urn:ngsi-ld:null"}));
    let instance = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&fragment).expect("JSON")).expect("an instance");
    let error = instance
        .admit_null_placements(&attribute, NullSentinelAdmission::TemporalEvolution)
        .expect_err("the third prohibition");
    assert!(matches!(error, EntityRefusal::NestedNullValue { .. }), "{error:?}");
}

/// Clauses 5.5.8 and 5.5.12: a `datasetId` cannot be deleted by the sentinel.
#[test]
fn a_dataset_id_of_the_null_sentinel_is_refused_under_every_admission() {
    let member = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 9}], properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:null"}]}]);
    for admission in ADMISSIONS {
        let error = admit(&entity_with(&member), admission).expect_err("a datasetId cannot be deleted by the sentinel");
        assert!(error.to_string().contains("datasetId"), "{admission:?}: {error}");
    }
}

#[test]
fn an_ordinary_dataset_id_is_accepted_under_every_admission() {
    let member = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 9}], properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:A"}]}]);
    for admission in ADMISSIONS {
        admit(&entity_with(&member), admission).expect("an ordinary datasetId is well formed");
    }
}

/// A sub-attribute is a Property in its own right, so the value-position
/// prohibition reaches it too.
#[test]
fn a_null_sub_attribute_value_is_refused_outside_a_fragment() {
    let member = json!([{
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [{"@value": 9}],
        "https://example.org/accuracy": [{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]
    }]);
    let error = admit(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a null sub-attribute value here");
    assert!(error.to_string().contains("accuracy"), "{error}");
}

#[test]
fn an_ordinary_property_value_is_accepted_under_every_admission() {
    let member = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "office"}]}]);
    for admission in ADMISSIONS {
        admit(&entity_with(&member), admission).expect("an ordinary value is well formed");
    }
}

/// Clause 5.6.19 takes a bare attribute fragment.
#[test]
fn an_attribute_fragment_value_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
    let fragment = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]);
    let error = admit_fragment(&fragment, NullSentinelAdmission::Refused).expect_err("Replace Attribute does not support nulls");
    assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    admit_fragment(&fragment, NullSentinelAdmission::PartialUpdateFragment).expect("clause 5.6.4 deletes the selected instance with the sentinel");
}

#[test]
fn an_attribute_fragment_dataset_id_of_the_null_sentinel_is_refused() {
    let fragment = json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": 9}], properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:null"}]}]);
    for admission in ADMISSIONS {
        let error = admit_fragment(&fragment, admission).expect_err("a datasetId cannot be deleted by the sentinel");
        assert!(error.to_string().contains("datasetId"), "{admission:?}: {error}");
    }
}

/// Every family spelling of the deletion (clauses 4.5.18, 4.5.21.2, 4.5.22.2,
/// 4.5.3.2) is refused by a creation.
#[test]
fn a_creation_refuses_every_family_spelling_of_the_sentinel() {
    let cases = [
        json!({"@type": [AttributeType::LanguageProperty.iri().as_str()], properties::LANGUAGE_MAP.as_str(): [{"@value": "urn:ngsi-ld:null"}]}),
        json!({"@type": [AttributeType::ListProperty.iri().as_str()], properties::VALUE_LIST.as_str(): [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}),
        json!({"@type": [AttributeType::ListRelationship.iri().as_str()], properties::OBJECT_LIST.as_str(): [{"@list": [{"@id": "urn:ngsi-ld:null"}]}]}),
        json!({"@type": [AttributeType::ListRelationship.iri().as_str()], properties::OBJECT_LIST.as_str(): [{"@list": [{properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]}]}]}),
        json!({"@type": [AttributeType::Relationship.iri().as_str()], properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]}),
    ];
    for instance in cases {
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("a creation admits no NGSI-LD Null");
        assert!(matches!(error, EntityRefusal::ReservedNullValue { .. }), "{instance}: {error:?}");
    }
}

/// Clause 5.5.4: `scope` is a first-level member, so its Null is admitted in
/// a fragment only.
#[test]
fn the_null_scope_is_refused_outside_a_fragment_and_admitted_inside_one() {
    let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", properties::SCOPE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]);
    let error = admit(&document, NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a first level null here");
    assert!(error.to_string().contains("NGSI-LD Null"), "{error}");
    for admission in [NullSentinelAdmission::PartialUpdateFragment, NullSentinelAdmission::MergeFragment] {
        admit(&document, admission).expect("a fragment deletes the scope with the sentinel");
    }
}

/// Clause 4.18 admits the sentinel only "in case of deleted scopes".
#[test]
fn a_null_beside_a_scope_path_is_refused_under_every_admission() {
    let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", properties::SCOPE.as_str(): [{"@value": "/Madrid"}, {"@value": "urn:ngsi-ld:null"}]}]);
    for admission in ADMISSIONS {
        let error = admit(&document, admission).expect_err("the sentinel cannot stand beside a path");
        assert!(error.to_string().contains("deleted scope"), "{admission:?}: {error}");
    }
}

/// A scope level beyond what a label-path index holds is bad input.
#[test]
fn a_scope_level_beyond_the_storable_bound_is_refused_under_every_admission() {
    let level = format!("/{}", "a".repeat(1001));
    let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", properties::SCOPE.as_str(): [{"@value": level}]}]);
    for admission in ADMISSIONS {
        let error = admit(&document, admission).expect_err("the level cannot be stored");
        assert!(error.to_string().contains("scope level"), "{admission:?}: {error}");
    }
}

/// An identifier one octet past the bound is refused, not read as absent.
#[test]
fn an_over_long_entity_identifier_is_refused() {
    let long_id = format!("urn:ngsi-ld:Device:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
    let error = admit(&json!([{"@id": long_id}]), NullSentinelAdmission::Refused).expect_err("the identifier is too long");
    assert!(matches!(error, EntityRefusal::EntityId { .. }), "{error:?}");
}

/// A `GeoProperty` vertex past a pole is refused with the attribute and the
/// position (clause 5.5.4, RFC 7946 section 4), for every instance.
#[test]
fn a_location_vertex_past_a_pole_is_refused_with_the_attribute_and_the_vertex() {
    let instance = |dataset: Option<&str>| {
        let mut instance = json!({
            "@type": [AttributeType::GeoProperty.iri().as_str()],
            properties::VALUE.as_str(): [{"@type": ["https://purl.org/geojson/vocab#Polygon"], "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@list": [
                {"@list": [{"@value": 0}, {"@value": 0}]}, {"@list": [{"@value": 1}, {"@value": 95}]}, {"@list": [{"@value": 1}, {"@value": 1}]}, {"@list": [{"@value": 0}, {"@value": 0}]}
            ]}]}]}]
        });
        if let Some(dataset) = dataset {
            instance[properties::DATASET_ID.as_str()] = json!([{"@id": dataset}]);
        }
        instance
    };
    for dataset in [None, Some("urn:ngsi-ld:Dataset:a")] {
        let document = json!([{"@id": "urn:ngsi-ld:Device:1", properties::LOCATION.as_str(): [instance(dataset)]}]);
        let error = admit(&document, NullSentinelAdmission::Refused).expect_err("latitude 95 is no position");
        let message = error.to_string();
        assert!(message.starts_with("GeoProperty 'location'"), "{message}");
        assert!(message.contains("latitude 95") && message.contains("coordinates[0][1]"), "{message}");
    }
}

/// A string geometry is admitted only when it parses into a geometry object,
/// and the refusal does not echo the string (clause 4.7.2).
#[test]
fn a_string_geometry_is_admitted_only_when_it_parses_into_a_geometry_object() {
    let geo = |text: &str| json!([{"@id": "urn:ngsi-ld:Device:1", properties::LOCATION.as_str(): [{"@type": [AttributeType::GeoProperty.iri().as_str()], properties::VALUE.as_str(): [{"@value": text}]}]}]);
    admit(&geo("{\"type\":\"Point\",\"coordinates\":[1,2]}"), NullSentinelAdmission::Refused).expect("a point");
    let error = admit(&geo("<secret> not json"), NullSentinelAdmission::Refused).expect_err("not a geometry");
    assert!(!error.to_string().contains("<secret>"), "{error}");
    admit(&geo("urn:ngsi-ld:null"), NullSentinelAdmission::PartialUpdateFragment).expect("the NGSI-LD Null is no string geometry but a deletion");
}

/// One expanded Property instance holding `value`, optionally naming a
/// `datasetId`.
fn property_instance(value: i64, dataset_id: Option<&str>) -> Value {
    let mut instance = json!({"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": value}]});
    if let Some(dataset_id) = dataset_id {
        instance[properties::DATASET_ID.as_str()] = json!([{"@id": dataset_id}]);
    }
    instance
}

/// `instance` carrying the sub-attribute `sub_attribute` with `instances`.
fn with_sub_attribute(mut instance: Value, sub_attribute: &str, instances: Vec<Value>) -> Value {
    instance[sub_attribute] = Value::Array(instances);
    instance
}

/// Clause 4.5.5.1 identifies the instances of a sub-attribute by `datasetId`
/// as it does an attribute's, so two sharing one are refused at any depth.
#[test]
fn a_repeated_sub_attribute_dataset_id_is_refused_at_any_depth() {
    let instance = with_sub_attribute(
        property_instance(20, None),
        "https://example.org/accuracy",
        vec![
            property_instance(1, Some("urn:ngsi-ld:Dataset:A")),
            property_instance(2, Some("urn:ngsi-ld:Dataset:A")),
        ],
    );
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::MergeFragment).expect_err("one instance per datasetId");
    assert!(
        matches!(&error, EntityRefusal::RepeatedInstance { attribute, dataset_id }
            if attribute.as_str() == "https://example.org/accuracy" && dataset_id.as_str() == "urn:ngsi-ld:Dataset:A"),
        "{error:?}"
    );
}

/// Clause 4.5.5.1: "There can only be one default Attribute instance for an
/// Attribute with a given Attribute name in any request", whatever the depth.
#[test]
fn a_repeated_default_sub_sub_attribute_is_refused() {
    let accuracy = with_sub_attribute(
        property_instance(1, None),
        "https://example.org/method",
        vec![property_instance(1, None), property_instance(2, None)],
    );
    let instance = with_sub_attribute(property_instance(20, None), "https://example.org/accuracy", vec![accuracy]);
    for admission in ADMISSIONS {
        let error = admit(&entity_with_instance(&instance), admission).expect_err("one default instance per sub-attribute name");
        assert!(
            matches!(&error, EntityRefusal::MultipleDefaultInstances { attribute } if attribute.as_str() == "https://example.org/method"),
            "{admission:?}: {error:?}"
        );
    }
}

/// Clause 4.5.5.1: a default instance beside named ones is no repetition.
#[test]
fn distinct_sub_attribute_dataset_ids_are_admitted() {
    let instance = with_sub_attribute(
        property_instance(20, None),
        "https://example.org/accuracy",
        vec![
            property_instance(1, Some("urn:ngsi-ld:Dataset:A")),
            property_instance(2, Some("urn:ngsi-ld:Dataset:B")),
            property_instance(3, None),
        ],
    );
    for admission in ADMISSIONS {
        admit(&entity_with_instance(&instance), admission).expect("distinct sub-attribute instance identities are well formed");
    }
}

/// The bare attribute fragment of clause 5.6.4 is held to clause 4.5.5.1 at
/// every depth below it.
#[test]
fn a_partial_update_fragment_with_a_repeated_nested_dataset_id_is_refused() {
    let accuracy = with_sub_attribute(
        property_instance(1, None),
        "https://example.org/method",
        vec![
            property_instance(1, Some("urn:ngsi-ld:Dataset:A")),
            property_instance(2, Some("urn:ngsi-ld:Dataset:A")),
        ],
    );
    let fragment = json!([with_sub_attribute(property_instance(20, None), "https://example.org/accuracy", vec![accuracy])]);
    let error = admit_fragment(&fragment, NullSentinelAdmission::PartialUpdateFragment).expect_err("one instance per datasetId");
    assert!(
        matches!(&error, EntityRefusal::RepeatedInstance { attribute, .. } if attribute.as_str() == "https://example.org/method"),
        "{error:?}"
    );
}

/// Clause 5.5.8 patches the members a Fragment names, and clause 5.4 makes the
/// Fragment a JSON Merge Patch that names none of them mandatory: a partial
/// update admits a Fragment naming nothing, or only its `datasetId`.
#[test]
fn a_partial_update_admits_a_fragment_naming_only_its_dataset_id_or_nothing() {
    let fragments = [json!([{}]), json!([{properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:A"}]}])];
    for fragment in fragments {
        admit_fragment_as(&fragment, NullSentinelAdmission::PartialUpdateFragment, InstanceCompleteness::Partial)
            .unwrap_or_else(|error| panic!("{fragment}: {error:?}"));
    }
}

/// Clauses 5.2.5 and 5.6.19.4: a replacement is a whole instance, so one with
/// no value member is refused with its family's missing-value refusal, a
/// Fragment declaring no family being read as a Property (clause 4.5.2); a
/// Fragment declaring no family is admitted as a partial update.
#[test]
fn a_replacement_without_a_value_member_is_refused() {
    let unit_code = json!([{properties::UNIT_CODE.as_str(): [{"@value": "C62"}]}]);
    let typed_property = json!([{"@type": [AttributeType::Property.iri().as_str()]}]);
    let typed_relationship = json!([{"@type": [AttributeType::Relationship.iri().as_str()]}]);
    let empty = json!([{}]);

    for fragment in [&unit_code, &typed_property, &empty] {
        let error = admit_fragment_as(fragment, NullSentinelAdmission::Refused, InstanceCompleteness::Complete).expect_err("the value is missing");
        assert!(
            matches!(&error, EntityRefusal::MissingValue { attribute_type: AttributeType::Property, attribute } if attribute.as_str() == "https://example.org/member"),
            "{fragment}: {error:?}"
        );
    }
    let error = admit_fragment_as(&typed_relationship, NullSentinelAdmission::Refused, InstanceCompleteness::Complete).expect_err("the object is missing");
    assert!(
        matches!(&error, EntityRefusal::MissingObject { attribute } if attribute.as_str() == "https://example.org/member"),
        "{error:?}"
    );

    for fragment in [&unit_code, &empty] {
        admit_fragment_as(fragment, NullSentinelAdmission::PartialUpdateFragment, InstanceCompleteness::Partial)
            .unwrap_or_else(|error| panic!("{fragment}: {error:?}"));
    }
}

/// A replacement carrying its value is complete, whatever else it omits.
#[test]
fn a_replacement_carrying_its_value_is_admitted() {
    admit_fragment_as(
        &json!([property_instance(1, None)]),
        NullSentinelAdmission::Refused,
        InstanceCompleteness::Complete,
    )
    .expect("a Property with its value is a complete instance");
}

/// A Property instance whose value is the expanded value node `value`.
fn property_valued(value: &Value) -> Value {
    json!({
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [value],
    })
}

/// The expanded `DateTime`, `Date` and `Time` datatype IRIs, each with a
/// spelling clause 4.6.3 refuses for it.
fn misspelled_time_values() -> [(&'static str, &'static str); 7] {
    [
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00:00+01:00"),
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00:00+00:00"),
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00:00.1234567Z"),
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00Z"),
        ("https://uri.etsi.org/ngsi-ld/DateTime", "yesterday"),
        ("https://uri.etsi.org/ngsi-ld/Date", "2024-1-1"),
        ("https://uri.etsi.org/ngsi-ld/Time", "12:00:00+01:00"),
    ]
}

/// Clause 4.6.3 fixes each time value form, and annex C.6 makes the
/// `@value`/`@type` pair how a value declares one, so a declared value that
/// is not of its form is refused rather than stored as a string.
#[test]
fn a_declared_time_value_not_of_its_form_is_refused() {
    for (datatype, text) in misspelled_time_values() {
        let instance = property_valued(&json!({"@value": text, "@type": datatype}));
        for admission in ADMISSIONS {
            let error = admit(&entity_with_instance(&instance), admission).expect_err("a misspelled time value");
            assert!(matches!(error, EntityRefusal::Datatype { .. }), "{datatype} {text} {admission:?}: {error:?}");
        }
        let error = admit_fragment(&instance, NullSentinelAdmission::PartialUpdateFragment).expect_err("a misspelled time value");
        assert!(matches!(error, EntityRefusal::Datatype { .. }), "{datatype} {text}: {error:?}");
    }
}

/// A sub-attribute is an attribute of its instance (clause 4.5.2.2), and a
/// structured value nests values of its own, so the rule holds at every depth.
#[test]
fn a_declared_time_value_is_held_to_its_form_at_every_depth() {
    let misspelled = json!({"@value": "2024-01-01T00:00:00+01:00", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"});
    let mut on_a_sub_attribute = property_valued(&json!({"@value": 1}));
    on_a_sub_attribute["https://example.org/measuredAt"] = json!([property_valued(&misspelled)]);
    let inside_a_structure = property_valued(&json!({"https://example.org/reading": [{"https://example.org/at": [misspelled]}]}));
    let inside_a_list = property_valued(&json!({"@list": [misspelled]}));
    for instance in [on_a_sub_attribute, inside_a_structure, inside_a_list] {
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("a misspelled time value");
        assert!(matches!(error, EntityRefusal::Datatype { .. }), "{instance}: {error:?}");
    }
}

/// Clause 4.6.3 admits a comma for the decimal point and up to six digits.
#[test]
fn a_declared_time_value_of_its_form_is_admitted() {
    for (datatype, text) in [
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00:00Z"),
        ("https://uri.etsi.org/ngsi-ld/DateTime", "2024-01-01T00:00:00,123456Z"),
        ("https://uri.etsi.org/ngsi-ld/Date", "2024-01-01"),
        ("https://uri.etsi.org/ngsi-ld/Time", "12:00:00.5Z"),
    ] {
        let instance = property_valued(&json!({"@value": text, "@type": datatype}));
        admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).unwrap_or_else(|error| panic!("{datatype} {text}: {error:?}"));
    }
}

/// Clause 4.6.3 admits every JSON native type as a value, so a string that
/// declares no form is a string whatever it resembles.
#[test]
fn an_undeclared_string_resembling_a_time_value_is_admitted() {
    let instance = property_valued(&json!({"@value": "2024-01-01T00:00:00+01:00"}));
    admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect("an undeclared string is a string");
}

/// One expanded Property instance holding `value` beside the `valueType` `value_type`.
fn property_with_value_type(value: &Value, value_type: &str) -> Value {
    json!({
        "@type": [AttributeType::Property.iri().as_str()],
        properties::VALUE.as_str(): [value],
        properties::VALUE_TYPE.as_str(): [{"@id": value_type}],
    })
}

/// Clause 4.6.3 writes each time value as a string, so a time datatype on a
/// number declares a value that is none, wherever it stands.
#[test]
fn a_time_datatype_on_a_number_is_refused_at_every_depth() {
    let numeric = json!({"@value": 123, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"});
    let at_top = property_valued(&numeric);
    let mut on_a_sub_attribute = property_valued(&json!({"@value": 1}));
    on_a_sub_attribute["https://example.org/measuredAt"] = json!([property_valued(&numeric)]);
    let inside_a_structure = property_valued(&json!({"https://example.org/reading": [{"https://example.org/at": [numeric]}]}));
    for instance in [at_top, on_a_sub_attribute, inside_a_structure] {
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("a numeric time value");
        assert!(matches!(error, EntityRefusal::Datatype { .. }), "{instance}: {error:?}");
    }
}

/// Annex C.6: `valueType` declares the datatype of the value beside it.
#[test]
fn a_value_type_holds_a_plain_value_to_its_datatype() {
    let declared = property_with_value_type(&json!({"@value": "yesterday"}), "https://uri.etsi.org/ngsi-ld/DateTime");
    let error = admit(&entity_with_instance(&declared), NullSentinelAdmission::Refused).expect_err("not a DateTime");
    assert!(matches!(error, EntityRefusal::Datatype { .. }), "{error:?}");
    let undeclared = property_valued(&json!({"@value": "yesterday"}));
    admit(&entity_with_instance(&undeclared), NullSentinelAdmission::Refused).expect("an undeclared string is a string");
}

/// Clause 4.5.2 aligns a JSON primitive's `valueType` with its RDF datatype,
/// and a value outside a recognised datatype is not one of its values.
#[test]
fn a_value_outside_its_recognised_xsd_datatype_is_refused() {
    let integer = "http://www.w3.org/2001/XMLSchema#integer";
    let list_item = json!({
        "@type": [AttributeType::ListProperty.iri().as_str()],
        properties::VALUE_LIST.as_str(): [{"@list": [{"@value": 300}, {"@value": 1.5}]}],
        properties::VALUE_TYPE.as_str(): [{"@id": integer}],
    });
    let mut typed_on_a_sub_attribute = property_valued(&json!({"@value": 1}));
    typed_on_a_sub_attribute["https://example.org/count"] = json!([property_valued(&json!({"@value": "abc", "@type": integer}))]);
    for instance in [
        property_with_value_type(&json!({"@value": "abc"}), integer),
        property_with_value_type(&json!({"@value": 1.5}), integer),
        property_with_value_type(&json!({"@value": 42}), "http://www.w3.org/2001/XMLSchema#string"),
        list_item,
        typed_on_a_sub_attribute,
    ] {
        let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).expect_err("outside its datatype");
        assert!(matches!(error, EntityRefusal::Datatype { .. }), "{instance}: {error:?}");
    }
}

/// RDF 1.1 Concepts section 5.3: an unrecognised datatype IRI has nothing to
/// check, which keeps annex C's `Integer` and C.10's `PostalAddress` valid.
#[test]
fn a_value_type_outside_the_recognised_set_is_admitted_unchecked() {
    let annex_c = json!({
        "@type": [AttributeType::ListProperty.iri().as_str()],
        properties::VALUE_LIST.as_str(): [{"@list": [{"@value": 300}, {"@value": 300}, {"@value": 120}, {"@value": 6}]}],
        properties::VALUE_TYPE.as_str(): [{"@id": "https://uri.etsi.org/ngsi-ld/default-context/Integer"}],
    });
    let postal_address = property_with_value_type(
        &json!({"https://example.org/street": [{"@value": "Pariser Platz"}]}),
        "http://example.org/PostalAddress",
    );
    let xsd_date_time = property_with_value_type(&json!({"@value": "2024-01-01T01:00:00+01:00"}), "http://www.w3.org/2001/XMLSchema#dateTime");
    for instance in [annex_c, postal_address, xsd_date_time] {
        admit(&entity_with_instance(&instance), NullSentinelAdmission::Refused).unwrap_or_else(|error| panic!("{instance}: {error:?}"));
    }
}

/// Table 5.2.32-1: the `valueType` of a `LanguageProperty` "shall be equal to
/// `langString`".
#[test]
fn a_language_property_value_type_other_than_lang_string_is_refused() {
    let language_property = |value_type: &str| {
        json!({
            "@type": [AttributeType::LanguageProperty.iri().as_str()],
            properties::LANGUAGE_MAP.as_str(): [{"@value": "Grand Place", "@language": "en"}],
            properties::VALUE_TYPE.as_str(): [{"@id": value_type}],
        })
    };
    admit(
        &entity_with_instance(&language_property("http://www.w3.org/1999/02/22-rdf-syntax-ns#langString")),
        NullSentinelAdmission::Refused,
    )
    .expect("langString is the LanguageProperty valueType");
    let error = admit(
        &entity_with_instance(&language_property("http://www.w3.org/2001/XMLSchema#string")),
        NullSentinelAdmission::Refused,
    )
    .expect_err("only langString");
    assert!(matches!(error, EntityRefusal::Datatype { .. }), "{error:?}");
}
