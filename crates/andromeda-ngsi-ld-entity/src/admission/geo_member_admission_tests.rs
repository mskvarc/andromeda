//! The family rule of the geospatial members, on whole documents and on bare
//! attribute fragments.

use crate::{
    admission::{instance_completeness::InstanceCompleteness, null_sentinel_admission::NullSentinelAdmission, written_families::WrittenFamilies},
    document::{EntityDocument, instance_document::InstanceDocument},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::Iri;
use serde_json::{Value, json};

/// The three members Table 5.2.4-1 types as `GeoProperty`.
const GEO_MEMBERS: [Iri<&'static str>; 3] = [properties::LOCATION, properties::OBSERVATION_SPACE, properties::OPERATION_SPACE];

/// One expanded entity holding `instances` under the expanded name `name`.
fn entity_with(name: &str, instances: &Value) -> Value {
    json!([{ "@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"], name: instances }])
}

/// One expanded instance of `family`, holding a value the family carries.
fn instance_of(family: AttributeType) -> Value {
    json!({"@type": [family.iri().as_str()], family.value_key_str(): value_of(family)})
}

/// The value member each family carries, in its expanded shape.
fn value_of(family: AttributeType) -> Value {
    match family {
        AttributeType::Property => json!([{"@value": "somewhere"}]),
        AttributeType::GeoProperty => json!([{"@value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}]),
        AttributeType::Relationship | AttributeType::VocabProperty => json!([{"@id": "urn:ngsi-ld:Place:1"}]),
        AttributeType::LanguageProperty => json!([{"@value": "somewhere", "@language": "en"}]),
        AttributeType::JsonProperty => json!([{"@type": "@json", "@value": {"place": "somewhere"}}]),
        AttributeType::ListProperty => json!([{"@list": [{"@value": "somewhere"}]}]),
        AttributeType::ListRelationship => json!([{"@list": [{"@id": "urn:ngsi-ld:Place:1"}]}]),
    }
}

/// Decode `document` and admit it under `admission` with `families`.
fn admit(document: &Value, admission: NullSentinelAdmission, families: WrittenFamilies) -> Result<(), EntityRefusal> {
    let bytes = serde_json::to_vec(document).expect("JSON");
    EntityDocument::from_slice(&bytes)?.admit(admission, families)
}

/// Decode `fragment`, written for the expanded name `name`, and admit it as
/// the bare fragment of a partial update.
fn admit_fragment(name: &str, fragment: &Value) -> Result<(), EntityRefusal> {
    let attribute: AttributeName = name.parse().expect("a name");
    let bytes = serde_json::to_vec(fragment).expect("JSON");
    InstanceDocument::from_slice(&attribute, &bytes)?.admit_fragment(&attribute, NullSentinelAdmission::PartialUpdateFragment, InstanceCompleteness::Partial)
}

/// Assert `result` is the refusal of `member` written as `family`.
fn assert_not_a_geo_property(result: Result<(), EntityRefusal>, member: Iri<&'static str>, family: AttributeType) {
    let Err(EntityRefusal::NotAGeoProperty { attribute, family: written }) = result else {
        panic!("{member} as a {family}: {result:?}");
    };
    pretty_assertions::assert_eq!(attribute.as_str(), member.as_str());
    pretty_assertions::assert_eq!(written, family);
}

#[test]
fn each_geospatial_member_written_as_another_family_is_refused() {
    for member in GEO_MEMBERS {
        for family in AttributeType::ALL.into_iter().filter(|family| *family != AttributeType::GeoProperty) {
            let document = entity_with(member.as_str(), &json!([instance_of(family)]));
            assert_not_a_geo_property(admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final), member, family);
        }
    }
}

#[test]
fn each_geospatial_member_written_as_a_geo_property_is_admitted() {
    for member in GEO_MEMBERS {
        let document = entity_with(member.as_str(), &json!([instance_of(AttributeType::GeoProperty)]));
        admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final).expect("a GeoProperty");
    }
}

#[test]
fn the_refusal_names_the_member_by_its_core_term_and_the_family_written() {
    let document = entity_with(properties::OBSERVATION_SPACE.as_str(), &json!([instance_of(AttributeType::Relationship)]));
    let error = admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final).expect_err("a Relationship");
    pretty_assertions::assert_eq!(error.to_string(), "'observationSpace' must be a GeoProperty, not a Relationship");
}

#[test]
fn one_property_instance_among_geo_property_instances_is_refused() {
    let mut tagged = instance_of(AttributeType::Property);
    tagged[properties::DATASET_ID.as_str()] = json!([{"@id": "urn:ngsi-ld:Dataset:gps"}]);
    let document = entity_with(properties::LOCATION.as_str(), &json!([instance_of(AttributeType::GeoProperty), tagged]));
    assert_not_a_geo_property(
        admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final),
        properties::LOCATION,
        AttributeType::Property,
    );
}

/// Clause 4.5.2.3 reads an instance that declares no family as the Property
/// its `value` member implies.
#[test]
fn a_typeless_instance_with_a_plain_value_is_a_property_and_refused() {
    let document = entity_with(properties::LOCATION.as_str(), &json!([{properties::VALUE.as_str(): [{"@value": "somewhere"}]}]));
    assert_not_a_geo_property(
        admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final),
        properties::LOCATION,
        AttributeType::Property,
    );
}

/// Clauses 5.5.8 and 5.6.4: an instance naming neither a family nor a value
/// keeps the family of the instance it updates.
#[test]
fn an_instance_naming_neither_a_family_nor_a_value_is_left_to_the_held_family() {
    let document = entity_with(
        properties::LOCATION.as_str(),
        &json!([{properties::OBSERVED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2026-01-01T00:00:00Z"}]}]),
    );
    admit(&document, NullSentinelAdmission::PartialUpdateFragment, WrittenFamilies::Final).expect("no family is written");
}

#[test]
fn a_value_deleted_with_the_ngsi_ld_null_writes_no_family() {
    let document = entity_with(
        properties::LOCATION.as_str(),
        &json!([{"@type": [AttributeType::Property.iri().as_str()], properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]),
    );
    admit(&document, NullSentinelAdmission::PartialUpdateFragment, WrittenFamilies::Final).expect("a deletion");
}

#[test]
fn an_attribute_named_location_in_another_vocabulary_may_be_any_family() {
    let document = entity_with("https://schema.org/location", &json!([instance_of(AttributeType::Property)]));
    admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final).expect("not the core location");
}

/// Table 5.2.4-1 types the members of the entity, not the sub-attributes of
/// an instance.
#[test]
fn a_sub_attribute_named_location_may_be_any_family() {
    let mut instance = instance_of(AttributeType::Property);
    instance[properties::LOCATION.as_str()] = json!([instance_of(AttributeType::Property)]);
    let document = entity_with("https://example.org/address", &json!([instance]));
    admit(&document, NullSentinelAdmission::Refused, WrittenFamilies::Final).expect("a sub-attribute");
}

/// Table 6.5.3.4-1: a simplified merge fragment takes the types the target
/// holds once rewritten against it, so its families are judged then.
#[test]
fn a_fragment_whose_families_the_target_decides_is_judged_once_rewritten() {
    let document = entity_with(properties::LOCATION.as_str(), &json!([instance_of(AttributeType::Property)]));
    admit(&document, NullSentinelAdmission::MergeFragment, WrittenFamilies::TargetHeld).expect("the target decides the family");
    assert_not_a_geo_property(
        admit(&document, NullSentinelAdmission::MergeFragment, WrittenFamilies::Final),
        properties::LOCATION,
        AttributeType::Property,
    );
}

#[test]
fn a_bare_fragment_for_a_geospatial_member_is_held_to_geo_property() {
    for member in GEO_MEMBERS {
        assert_not_a_geo_property(
            admit_fragment(member.as_str(), &instance_of(AttributeType::Property)),
            member,
            AttributeType::Property,
        );
        admit_fragment(member.as_str(), &instance_of(AttributeType::GeoProperty)).expect("a GeoProperty");
    }
}

#[test]
fn a_bare_fragment_for_another_attribute_may_be_any_family() {
    admit_fragment("https://example.org/address", &instance_of(AttributeType::Property)).expect("an ordinary attribute");
}
