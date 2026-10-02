//! The admission rules, with the cases the expanded-JSON admission pinned.

use crate::{
    admission::null_sentinel_admission::NullSentinelAdmission,
    document::{EntityDocument, instance_document::InstanceDocument},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier_length::IDENTIFIER_BYTE_LIMIT};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Value, json};

/// Every admission, so a case can state what each one does with a payload.
const ADMISSIONS: [NullSentinelAdmission; 3] = [
    NullSentinelAdmission::Refused,
    NullSentinelAdmission::PartialUpdateFragment,
    NullSentinelAdmission::MergeFragment,
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

/// Decode the attribute fragment `fragment` and admit it under `admission`.
fn admit_fragment(fragment: &Value, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
    let attribute: AttributeName = "https://example.org/member".parse().expect("a name");
    let bytes = serde_json::to_vec(fragment).expect("JSON");
    InstanceDocument::from_slice(&attribute, &bytes)?.admit_fragment(&attribute, admission)
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

#[test]
fn a_property_missing_its_value_is_refused() {
    let instance = json!({"@type": [AttributeType::Property.iri().as_str()]});
    let error = admit(&entity_with_instance(&instance), NullSentinelAdmission::MergeFragment).expect_err("clause 5.2.5 requires a value");
    assert!(error.to_string().contains("value"), "{error}");
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
