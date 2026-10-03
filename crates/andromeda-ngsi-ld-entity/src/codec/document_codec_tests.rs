//! Round trips and readings of written documents: the deletions a fragment
//! may carry (clauses 5.5.8, 5.5.12) and the Temporal Evolution members
//! (clause 4.5.7).

use crate::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    language_entries::LanguageEntry,
    refusal::EntityRefusal,
    value_mode::{FragmentMember, FragmentText},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Value, json};

const NGSI: &str = "https://uri.etsi.org/ngsi-ld/";
const NULL: &str = "urn:ngsi-ld:null";

/// The expanded IRI of an NGSI-LD core term.
fn ngsi(term: &str) -> String {
    format!("{NGSI}{term}")
}

/// Decode `document` as a written document.
fn decode(document: &Value) -> EntityDocument {
    EntityDocument::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a written document")
}

/// Decode, encode and parse back.
fn round_trip(document: &Value) -> Value {
    let encoded = decode(document).to_vec().expect("encodes");
    serde_json::from_slice(&encoded).expect("JSON")
}

/// The one written instance of `attribute`.
fn instance<'a>(document: &'a EntityDocument, attribute: &str) -> &'a InstanceDocument {
    document
        .attributes()
        .get(attribute)
        .and_then(|member| member.instances().first())
        .expect("a written instance")
}

#[test]
fn a_merge_fragment_with_every_deletion_round_trips() {
    let document = json!([{
        "@id": "urn:ngsi-ld:Building:1",
        ngsi("scope"): [{"@value": NULL}],
        ngsi("expiresAt"): [{"@type": ngsi("DateTime"), "@value": NULL}],
        "https://ex.org/gone": [{"@value": NULL}],
        "https://ex.org/address": [{
            "@type": [ngsi("Property")],
            ngsi("hasValue"): [{"https://ex.org/street": [{"@value": "Main"}], "https://ex.org/country": [{"@value": NULL}]}],
            ngsi("unitCode"): [{"@value": NULL}],
            ngsi("observedAt"): [{"@type": ngsi("DateTime"), "@value": NULL}],
            ngsi("hasValueType"): [{"@id": NULL}]
        }],
        "https://ex.org/name": [{
            "@type": [ngsi("LanguageProperty")],
            ngsi("hasLanguageMap"): [{"@value": "Station", "@language": "en"}, {"@value": NULL, "@language": "de"}]
        }],
        "https://ex.org/config": [{"@type": [ngsi("JsonProperty")], ngsi("hasJSON"): [{"@type": "@json", "@value": {"a": NULL, "b": null}}]}],
        "https://ex.org/route": [{"@type": [ngsi("ListRelationship")], ngsi("hasObjectList"): [{"@list": [{"@value": NULL}]}]}],
        "https://ex.org/owner": [{"@type": [ngsi("Relationship")], ngsi("hasObject"): [{"@id": NULL}]}],
        "https://ex.org/speed": [{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": NULL}], ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]}]
    }]);
    let written = decode(&document);
    assert!(matches!(written.scope(), Some(Slot::Remove)));
    assert!(matches!(written.expires_at(), Some(Slot::Remove)));
    assert!(matches!(written.attributes().get("https://ex.org/gone"), Some(AttributeMember::Remove)));
    let address = instance(&written, "https://ex.org/address");
    assert!(matches!(address.unit_code(), Some(Slot::Remove)));
    assert!(matches!(address.observed_at(), Some(Slot::Remove)));
    assert!(matches!(address.value_type(), Some(Slot::Remove)));
    let ValueSlot::Set(WrittenValue::Value(values)) = address.value() else {
        panic!("a written value");
    };
    let [ExpandedValue::Node(node)] = values.as_slice() else {
        panic!("a structured value");
    };
    assert!(matches!(node.members().get("https://ex.org/country"), Some(FragmentMember::Remove)));
    let ValueSlot::Set(WrittenValue::LanguageMap(entries)) = instance(&written, "https://ex.org/name").value() else {
        panic!("a language map");
    };
    assert!(matches!(entries.entries().get(1).map(LanguageEntry::text), Some(FragmentText::Remove)));
    assert!(matches!(
        instance(&written, "https://ex.org/route").value(),
        ValueSlot::Remove(ValueCarrier::ObjectList)
    ));
    assert!(matches!(
        instance(&written, "https://ex.org/owner").value(),
        ValueSlot::Remove(ValueCarrier::Object)
    ));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn a_typeless_fragment_carries_its_value_by_its_member() {
    let document = json!([{"https://ex.org/name": [{ngsi("hasLanguageMap"): [{"@value": "Grand Place", "@language": "en"}, {"@value": "Gran Lugar", "@language": "es"}]}]}]);
    let written = decode(&document);
    let name = instance(&written, "https://ex.org/name");
    pretty_assertions::assert_eq!(name.family(), None);
    assert!(matches!(name.value(), ValueSlot::Set(WrittenValue::LanguageMap(_))));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn a_temporal_evolution_fragment_keeps_repeated_datasets_instance_ids_and_deletions() {
    let observation = |value: i64, observed: &str, id: &str| {
        json!({
            "@type": [ngsi("Property")],
            ngsi("hasValue"): [{"@value": value}],
            ngsi("observedAt"): [{"@type": ngsi("DateTime"), "@value": observed}],
            ngsi("instanceId"): [{"@id": id}]
        })
    };
    let document = json!([{
        "@id": "urn:ngsi-ld:Vehicle:1",
        "@type": ["https://ex.org/Vehicle"],
        "https://ex.org/speed": [
            observation(1, "2026-01-01T00:00:00Z", "urn:ngsi-ld:Instance:1"),
            observation(2, "2026-01-01T00:01:00Z", "urn:ngsi-ld:Instance:2"),
            {"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": NULL}], ngsi("deletedAt"): [{"@type": ngsi("DateTime"), "@value": "2026-01-01T00:02:00Z"}]}
        ]
    }]);
    let written = decode(&document);
    pretty_assertions::assert_eq!(written.attributes().get("https://ex.org/speed").map(|member| member.instances().len()), Some(3));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn a_bare_attribute_fragment_round_trips() {
    let attribute: AttributeName = "https://ex.org/speed".parse().expect("a name");
    let fragment = json!([{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": 9}], ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]}]);
    let written = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&fragment).expect("JSON")).expect("a fragment");
    pretty_assertions::assert_eq!(written.family(), Some(AttributeType::Property));
    pretty_assertions::assert_eq!(written.dataset_id().map(Identifier::as_str), Some("urn:ngsi-ld:Dataset:a"));
    let encoded: Value = serde_json::from_slice(&written.to_vec().expect("encodes")).expect("JSON");
    pretty_assertions::assert_eq!(encoded, fragment);
    let bare = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&fragment[0]).expect("JSON")).expect("a bare fragment");
    pretty_assertions::assert_eq!(bare, written);
}

#[test]
fn a_null_beside_data_in_one_member_is_refused() {
    let document = json!([{"https://ex.org/speed": [{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": NULL}, {"@value": 5}]}]}]);
    let error = EntityDocument::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect_err("a null is no datum");
    assert!(matches!(error, EntityRefusal::NullBesideData { .. }), "{error:?}");
}

#[test]
fn an_output_only_member_is_refused() {
    let document = json!([{"https://ex.org/speed": [{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": 1}], properties::PREVIOUS_VALUE.as_str(): [{"@value": 0}]}]}]);
    let error = EntityDocument::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect_err("previousValue is output-only");
    assert!(matches!(error, EntityRefusal::OutputOnlyMember { .. }), "{error:?}");
}

#[test]
fn an_instance_that_is_a_bare_value_is_refused() {
    let document = json!([{"https://ex.org/speed": [{"@value": 5}]}]);
    let error = EntityDocument::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect_err("a value is no instance");
    assert!(matches!(error, EntityRefusal::Malformed { .. }), "{error:?}");
}

/// Clause 4.5.0 spells each family's deletion in its own value member; each
/// spelling reads as the deletion of its carrier and is written back as the
/// spelling the expansion of a request produces.
#[test]
fn every_family_encodes_its_deletion_the_way_clause_4_5_0_spells_it() {
    for family in AttributeType::ALL {
        let deletion = match family {
            AttributeType::Relationship | AttributeType::VocabProperty => json!([{"@id": NULL}]),
            AttributeType::JsonProperty => json!([{"@type": "@json", "@value": NULL}]),
            AttributeType::Property | AttributeType::GeoProperty | AttributeType::LanguageProperty => json!([{"@value": NULL}]),
            AttributeType::ListProperty | AttributeType::ListRelationship => json!([{"@list": [{"@value": NULL}]}]),
        };
        let document = json!([{"https://ex.org/a": [{"@type": [family.iri().as_str()], family.value_key_str(): deletion}]}]);
        let written = decode(&document);
        assert!(
            matches!(instance(&written, "https://ex.org/a").value(), ValueSlot::Remove(carrier) if *carrier == ValueCarrier::of(family)),
            "{family}"
        );
        pretty_assertions::assert_eq!(round_trip(&document), document, "{family}");
    }
}

/// Clause 4.5.22.2: a normalized object list of one Null is the deletion too.
#[test]
fn a_normalized_object_list_of_one_null_is_a_list_relationship_deletion() {
    let document = json!([{"https://ex.org/a": [{
        "@type": [AttributeType::ListRelationship.iri().as_str()],
        properties::OBJECT_LIST.as_str(): [{"@list": [{properties::OBJECT.as_str(): [{"@id": NULL}]}]}]
    }]}]);
    assert!(matches!(
        instance(&decode(&document), "https://ex.org/a").value(),
        ValueSlot::Remove(ValueCarrier::ObjectList)
    ));
}

/// A list holding the Null beside data is neither a deletion nor data.
#[test]
fn a_list_that_is_not_a_single_null_is_not_a_deletion() {
    let document = json!([{"https://ex.org/a": [{
        "@type": [AttributeType::ListProperty.iri().as_str()],
        properties::VALUE_LIST.as_str(): [{"@list": [{"@value": NULL}, {"@value": 1}]}]
    }]}]);
    let error = EntityDocument::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect_err("a mixed list");
    assert!(matches!(error, EntityRefusal::NullBesideData { .. }), "{error:?}");
}

/// Clause 4.18: there is at most one deleted scope to state.
#[test]
fn a_repeated_null_scope_is_refused() {
    let document = json!([{properties::SCOPE.as_str(): [{"@value": NULL}, {"@value": NULL}]}]);
    let error = EntityDocument::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect_err("two deletions");
    assert!(error.to_string().contains("more than once"), "{error}");
}

/// Clause 3.1: a written `hasValue: []` is the empty array, set rather than
/// absent, and is written back as it was read.
#[test]
fn a_written_empty_array_value_is_set_and_round_trips() {
    let document = json!([{"https://ex.org/tags": [{"@type": [ngsi("Property")], ngsi("hasValue"): []}]}]);
    let written = decode(&document);
    pretty_assertions::assert_eq!(
        instance(&written, "https://ex.org/tags").value(),
        &ValueSlot::Set(WrittenValue::Value(Vec::new()))
    );
    pretty_assertions::assert_eq!(round_trip(&document), document);
}
